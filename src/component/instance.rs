use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::component::Linker;

use crate::component::Component;
use crate::component::exports::Exports;
use crate::engine::engine;
use crate::error::link_error;
use crate::store::{self, HostState};
use crate::throw::call_error;

/// An instance of a component, with a store of its own.
#[php_class]
#[php(name = "Wasm\\Component\\Instance")]
#[php(flags = ClassFlags::Final)]
pub struct Instance {
    exports: Zval,
}

#[php_impl]
impl Instance {
    /// @param array<string, callable|array<string, callable>>|null $imports
    pub fn __construct(component: &Component, imports: Option<&ZendHashTable>) -> PhpResult<Self> {
        let _ = imports;
        let store = store::new();
        let linker: Linker<HostState> = Linker::new(engine());
        let exports = store.with(|mut ctx| {
            let instance = match linker.instantiate(&mut ctx, &component.inner) {
                Ok(instance) => instance,
                Err(err) if err.is::<wasmtime::Trap>() => return Err(call_error(&mut ctx, err)),
                Err(err) => return Err(link_error(err)),
            };
            let ty = component.inner.component_type();
            Exports::new(&store, &mut ctx, &instance, None, ty.exports(engine()))
        })?;
        Ok(Self {
            exports: exports.into_zval(false)?,
        })
    }

    #[php(getter)]
    pub fn get_exports(&self) -> Zval {
        self.exports.shallow_clone()
    }
}
