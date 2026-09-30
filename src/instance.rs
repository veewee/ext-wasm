use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::Extern;

use crate::error::{link_error, runtime_error};
use crate::imports;
use crate::exports::Exports;
use crate::module::Module;
use crate::store;

#[php_class]
#[php(name = "Wasm\\Instance")]
#[php(flags = ClassFlags::Final)]
pub struct Instance {
    exports: Zval,
}

#[php_impl]
impl Instance {
    pub fn __construct(module: &Module, imports: Option<&ZendHashTable>) -> PhpResult<Self> {
        let store = store::current();
        let imports = imports::resolve(&store, &module.inner, imports)?;
        let externs: Vec<(String, Extern)> = store.with(|mut ctx| {
            let instance = wasmtime::Instance::new(&mut ctx, &module.inner, &imports).map_err(instantiation_error)?;
            Ok::<_, ext_php_rs::exception::PhpException>(
                instance
                    .exports(&mut ctx)
                    .map(|export| (export.name().to_string(), export.into_extern()))
                    .collect(),
            )
        })?;
        let exports = Exports::new(store, externs)?.into_zval(false)?;
        Ok(Self { exports })
    }

    #[php(getter)]
    pub fn get_exports(&self) -> Zval {
        self.exports.shallow_clone()
    }
}

/// Traps raised by the start function are runtime errors, everything else is a link error.
fn instantiation_error(err: wasmtime::Error) -> ext_php_rs::exception::PhpException {
    if err.is::<wasmtime::Trap>() { runtime_error(err) } else { link_error(err) }
}
