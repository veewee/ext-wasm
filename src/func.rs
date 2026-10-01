use std::cell::{OnceCell, RefCell};
use std::rc::{Rc, Weak};

use ext_php_rs::boxed::ZBox;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::{FuncType, StoreContextMut, Val, ValType};

use crate::callback::host_func;
use crate::engine::engine;
use crate::error::{argument_count_error, type_error};
use crate::store::{self, HostState, SharedStore, StoreHandle};
use crate::suspend;
use crate::throw::call_error;
use crate::types::func_type;
use crate::value::{debug_type, default_val, parse_val_type, results_to_zval, to_val};

/// A wasm function: one a module exports, or a PHP callable of a given
/// function type, like JS `new WebAssembly.Function(type, fn)`.
#[php_class]
#[php(name = "Wasm\\Func")]
#[php(flags = ClassFlags::Final)]
pub struct Func {
    origin: Origin,
}

enum Origin {
    Wasm {
        store: SharedStore,
        inner: wasmtime::Func,
    },
    Host(HostFunc),
}

/// A PHP callable that belongs to no store. Each store it is used in gets a
/// wasm function of its own, as JS functions work with any instance.
struct HostFunc {
    ty: FuncType,
    callable: Zval,
    made: RefCell<Vec<(Weak<StoreHandle>, wasmtime::Func)>>,
    /// Where calls from PHP run.
    own: OnceCell<SharedStore>,
}

impl Func {
    pub fn wasm(store: SharedStore, inner: wasmtime::Func) -> Self {
        Self {
            origin: Origin::Wasm { store, inner },
        }
    }

    /// The store of an exported function. A PHP callable has none.
    pub fn store(&self) -> Option<&SharedStore> {
        match &self.origin {
            Origin::Wasm { store, .. } => Some(store),
            Origin::Host(_) => None,
        }
    }

    /// The wasm function to use in the store of `ctx`, or the store mismatch
    /// message for an exported function of another store.
    pub fn in_store(
        &self,
        ctx: &mut StoreContextMut<'_, HostState>,
    ) -> Result<wasmtime::Func, String> {
        match &self.origin {
            Origin::Wasm { store, inner } if store::owns(&*ctx, store) => Ok(*inner),
            Origin::Wasm { .. } => Err(store::mismatch_message("Func")),
            Origin::Host(host) => {
                let store = store::of(&*ctx);
                let mut made = host.made.borrow_mut();
                made.retain(|(owner, _)| owner.strong_count() > 0);
                if let Some((_, func)) = made
                    .iter()
                    .find(|(owner, _)| std::ptr::eq(owner.as_ptr(), Rc::as_ptr(&store)))
                {
                    return Ok(*func);
                }
                let func = host_func(&mut *ctx, host.ty.clone(), &host.callable, false);
                made.push((Rc::downgrade(&store), func));
                // The callable now lives as long as the store, and may hold
                // objects of it, so later standalone objects get another store.
                store::retire_standalone(&store);
                Ok(func)
            }
        }
    }

    /// The type of a PHP callable, known without a store.
    pub fn host_type(&self) -> Option<&FuncType> {
        match &self.origin {
            Origin::Wasm { .. } => None,
            Origin::Host(host) => Some(&host.ty),
        }
    }

    fn ty(&self) -> FuncType {
        match &self.origin {
            Origin::Wasm { store, inner } => store.with(|ctx| inner.ty(&ctx)),
            Origin::Host(host) => host.ty.clone(),
        }
    }
}

#[php_impl]
impl Func {
    /// @param array{parameters: list<string>, results: list<string>} $type
    /// @param callable $callback
    pub fn __construct(r#type: &ZendHashTable, callback: &Zval) -> PhpResult<Self> {
        let types = |key: &str| {
            r#type
                .get(key)
                .and_then(Zval::array)
                .ok_or_else(|| type_error(format!("type \"{key}\" must be a list of value types")))?
                .values()
                .map(|ty| {
                    ty.str()
                        .ok_or_else(|| {
                            type_error(format!(
                                "expected a value type name, got {}",
                                debug_type(ty)
                            ))
                        })
                        .and_then(parse_val_type)
                })
                .collect::<PhpResult<Vec<ValType>>>()
        };
        let (params, results) = (types("parameters")?, types("results")?);
        if !callback.is_callable() {
            return Err(type_error(format!(
                "expected a callable, got {}",
                debug_type(callback)
            )));
        }
        Ok(Self {
            origin: Origin::Host(HostFunc {
                ty: FuncType::new(engine(), params, results),
                callable: callback.shallow_clone(),
                made: RefCell::new(Vec::new()),
                own: OnceCell::new(),
            }),
        })
    }

    pub fn __invoke(&self, args: &[&Zval]) -> PhpResult<Zval> {
        match &self.origin {
            Origin::Wasm { store, inner } => call(store, inner, args),
            Origin::Host(host) => {
                let store = host.own.get_or_init(store::new).clone();
                let func = store
                    .with(|mut ctx| self.in_store(&mut ctx))
                    .map_err(crate::error::link_error)?;
                call(&store, &func, args)
            }
        }
    }

    /// @return array{parameters: list<string>, results: list<string>}
    pub fn r#type(&self) -> PhpResult<ZBox<ZendHashTable>> {
        func_type(&self.ty())
    }

    /// Number of parameters, like JS `Function.prototype.length`.
    pub fn length(&self) -> i64 {
        self.ty().params().len() as i64
    }
}

pub fn call(store: &SharedStore, func: &wasmtime::Func, args: &[&Zval]) -> PhpResult<Zval> {
    if store.is_parked() {
        return Err(store::busy());
    }
    store.with(|mut ctx| {
        let ty = func.ty(&ctx);
        let params: Vec<ValType> = ty.params().collect();
        if args.len() != params.len() {
            return Err(argument_count_error(format!(
                "wasm function expects {} argument(s), {} given",
                params.len(),
                args.len()
            )));
        }
        let args = args
            .iter()
            .zip(&params)
            .map(|(arg, ty)| to_val(&mut ctx, arg, ty))
            .collect::<Result<Vec<Val>, _>>()?;
        let mut results: Vec<Val> = ty.results().map(|ty| default_val(&ty)).collect();
        if let Err(err) = run(store, &mut ctx, func, &args, &mut results) {
            return Err(call_error(&mut ctx, err));
        }
        results_to_zval(&mut ctx, &results)
    })
}

/// Calls `func`, through `suspend::drive` when the store is async.
pub fn run(
    store: &StoreHandle,
    ctx: &mut StoreContextMut<'_, HostState>,
    func: &wasmtime::Func,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    if ctx.data().is_async {
        suspend::drive(store, func.call_async(&mut *ctx, params, results))
    } else {
        func.call(&mut *ctx, params, results)
    }
}
