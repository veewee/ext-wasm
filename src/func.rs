use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use wasmtime::{Val, ValType};

use crate::error::argument_count_error;
use crate::store::SharedStore;
use crate::throw::call_error;
use crate::value::{default_val, results_to_zval, to_val};

/// An exported wasm function, callable from PHP.
#[php_class]
#[php(name = "Wasm\\Func")]
#[php(flags = ClassFlags::Final)]
pub struct Func {
    pub store: SharedStore,
    pub inner: wasmtime::Func,
}

#[php_impl]
impl Func {
    pub fn __invoke(&self, args: &[&Zval]) -> PhpResult<Zval> {
        call(&self.store, &self.inner, args)
    }

    /// Number of parameters, like JS `Function.prototype.length`.
    pub fn length(&self) -> i64 {
        self.store
            .with(|ctx| self.inner.ty(&ctx).params().len() as i64)
    }
}

pub fn call(store: &SharedStore, func: &wasmtime::Func, args: &[&Zval]) -> PhpResult<Zval> {
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
        if let Err(err) = func.call(&mut ctx, &args, &mut results) {
            return Err(call_error(&mut ctx, err));
        }
        results_to_zval(&mut ctx, &results)
    })
}
