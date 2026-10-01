use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use wasmtime::component::Val;

use crate::component::host_resource;
use crate::component::resource;
use crate::component::value::{to_val, unwrap_result};
use crate::error::argument_count_error;
use crate::store::SharedStore;
use crate::throw::call_error;

/// An exported component function, callable from PHP.
#[php_class]
#[php(name = "Wasm\\Component\\Func")]
#[php(flags = ClassFlags::Final)]
pub struct Func {
    pub store: SharedStore,
    pub inner: wasmtime::component::Func,
    /// The function's type, built from the component's own types when the
    /// exports are, so it carries the names WIT gave them.
    pub signature: Zval,
}

#[php_impl]
impl Func {
    pub fn __invoke(&self, args: &[&Zval]) -> PhpResult<Zval> {
        self.call(args)
    }

    /// The function's WIT type.
    ///
    /// @return \Wasm\Component\Type\FunctionType
    pub fn r#type(&self) -> Zval {
        self.signature.shallow_clone()
    }
}

impl Clone for Func {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            inner: self.inner,
            signature: self.signature.shallow_clone(),
        }
    }
}

impl Func {
    pub fn call(&self, args: &[&Zval]) -> PhpResult<Zval> {
        self.call_with_self(None, args)
    }

    /// Calls with `first` as the first parameter, the handle of a resource
    /// method's `self`, and `args` for the rest.
    pub fn call_with_self(&self, first: Option<Val>, args: &[&Zval]) -> PhpResult<Zval> {
        if self.store.is_parked() {
            return Err(crate::store::busy());
        }
        self.store.with(|mut ctx| {
            let ty = self.inner.ty(&ctx);
            let params: Vec<_> = ty
                .params()
                .map(|(_, ty)| ty)
                .skip(usize::from(first.is_some()))
                .collect();
            if args.len() != params.len() {
                return Err(argument_count_error(format!(
                    "component function expects {} argument(s), {} given",
                    params.len(),
                    args.len()
                )));
            }
            let lent = host_resource::mark(&ctx);
            let moves = resource::moves_mark(&ctx);
            let converted = args
                .iter()
                .zip(&params)
                .map(|(arg, ty)| to_val(&mut ctx, arg, ty))
                .collect::<Result<Vec<Val>, _>>();
            let called = match converted {
                Ok(converted) => {
                    let params: Vec<Val> = first.into_iter().chain(converted).collect();
                    let mut results = vec![Val::Bool(false); ty.results().len()];
                    run(&self.store, &mut ctx, self.inner, &params, &mut results)
                        .map(|()| results)
                        .map_err(|err| call_error(&mut ctx, err))
                }
                Err(err) => Err(err.into()),
            };
            // Resources given for own parameters belong to the component only
            // once the call worked, and lent PHP objects always come back.
            resource::finish_moves(&mut ctx, moves, called.is_ok());
            let mut released = Vec::new();
            host_resource::reclaim(&mut ctx, lent, called.is_ok(), &mut released);
            for object in released {
                self.store.put_garbage(object);
            }
            let results = called?;
            let result_types: Vec<_> = ty.results().collect();
            match (results.first(), result_types.first()) {
                (Some(val), Some(ty)) => unwrap_result(&mut ctx, val, ty),
                _ => Ok(Zval::null()),
            }
        })
    }
}

/// Calls `func`, through `suspend::drive` when the store is async.
pub fn run(
    store: &crate::store::StoreHandle,
    ctx: &mut wasmtime::StoreContextMut<'_, crate::store::HostState>,
    func: wasmtime::component::Func,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    if ctx.data().is_async {
        crate::suspend::drive(store, func.call_async(&mut *ctx, params, results))
    } else {
        func.call(&mut *ctx, params, results)
    }
}
