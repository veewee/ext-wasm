use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use wasmtime::component::Val;

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
}

#[php_impl]
impl Func {
    pub fn __invoke(&self, args: &[&Zval]) -> PhpResult<Zval> {
        self.call(args)
    }
}

impl Func {
    pub fn call(&self, args: &[&Zval]) -> PhpResult<Zval> {
        self.call_with_self(None, args)
    }

    /// Calls with `first` as the first parameter, the handle of a resource
    /// method's `self`, and `args` for the rest.
    pub fn call_with_self(&self, first: Option<Val>, args: &[&Zval]) -> PhpResult<Zval> {
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
            let converted = args
                .iter()
                .zip(&params)
                .map(|(arg, ty)| to_val(&mut ctx, arg, ty))
                .collect::<Result<Vec<Val>, _>>();
            // Resources given for own parameters move only when the call runs.
            resource::commit_moves(converted.is_ok());
            let params: Vec<Val> = first.into_iter().chain(converted?).collect();
            let result_types: Vec<_> = ty.results().collect();
            let mut results = vec![Val::Bool(false); result_types.len()];
            if let Err(err) = self.inner.call(&mut ctx, &params, &mut results) {
                return Err(call_error(&mut ctx, err));
            }
            match (results.first(), result_types.first()) {
                (Some(val), Some(ty)) => unwrap_result(&mut ctx, val, ty),
                _ => Ok(Zval::null()),
            }
        })
    }
}
