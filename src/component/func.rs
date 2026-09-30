use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use wasmtime::component::Val;

use crate::component::value::{from_val, to_val};
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
}

#[php_impl]
impl Func {
    pub fn __invoke(&self, args: &[&Zval]) -> PhpResult<Zval> {
        self.call(args)
    }
}

impl Func {
    pub fn call(&self, args: &[&Zval]) -> PhpResult<Zval> {
        self.store.with(|mut ctx| {
            let ty = self.inner.ty(&ctx);
            let params: Vec<_> = ty.params().map(|(_, ty)| ty).collect();
            if args.len() != params.len() {
                return Err(argument_count_error(format!(
                    "component function expects {} argument(s), {} given",
                    params.len(),
                    args.len()
                )));
            }
            let params = args
                .iter()
                .zip(&params)
                .map(|(arg, ty)| to_val(arg, ty))
                .collect::<Result<Vec<Val>, _>>()?;
            let result_types: Vec<_> = ty.results().collect();
            let mut results = vec![Val::Bool(false); result_types.len()];
            if let Err(err) = self.inner.call(&mut ctx, &params, &mut results) {
                return Err(call_error(&mut ctx, err));
            }
            match (results.first(), result_types.first()) {
                (Some(val), Some(ty)) => Ok(from_val(val, ty)?),
                _ => Ok(Zval::null()),
            }
        })
    }
}
