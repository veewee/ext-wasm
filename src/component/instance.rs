use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::component::Linker;

use crate::component::Component;
use crate::component::exports::Exports;
use crate::component::http_handler::{self, Request, Response};
use crate::component::imports;
use crate::engine::engine;
use crate::error::{error, instantiation_error, link_error};
use crate::store::{self, HostState, SharedStore};
use crate::suspend;
use crate::throw::call_error;
use crate::value::downcast;
use crate::wasi::Wasi;

/// An instance of a component, with a store of its own.
#[php_class]
#[php(name = "Wasm\\Component\\Instance")]
#[php(flags = ClassFlags::Final)]
pub struct Instance {
    store: SharedStore,
    exports: Zval,
}

#[php_impl]
impl Instance {
    /// @param array<string, callable|array<string, callable>>|null $imports
    /// @param \Wasm\Wasi|null $wasi provides every `wasi:*` import, as WASI preview2
    pub fn __construct(
        component: &Component,
        imports: Option<&ZendHashTable>,
        wasi: Option<&Wasi>,
    ) -> PhpResult<Self> {
        let store = store::new();
        if imports::has_suspending(component, imports) || imports::uses_async_abi(component) {
            // Decided before linking: every PHP import of such an instance is
            // async. An async-ABI component runs every guest task on a fiber,
            // where PHP code must not run, so its plain imports go through the
            // poll loop on the PHP stack too.
            store.with(|mut ctx| ctx.data_mut().is_async = true);
        }
        let mut linker: Linker<HostState> = Linker::new(engine());
        imports::link(&store, &mut linker, component, imports)?;
        if let Some(wasi) = wasi {
            wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(link_error)?;
            if wasi.allows_http() {
                wasmtime_wasi_http::p2::add_only_http_to_linker_sync(&mut linker)
                    .map_err(link_error)?;
            }
            // The context builds only once, so it is taken after linking succeeded.
            linker
                .instantiate_pre(&component.inner)
                .map_err(link_error)?;
            wasi.attach(&store)?;
        }
        let exports = store.with(|mut ctx| {
            ctx.data_mut().memory.reset();
            let instantiated = if ctx.data().is_async {
                suspend::drive(&store, linker.instantiate_async(&mut ctx, &component.inner))
            } else {
                linker.instantiate(&mut ctx, &component.inner)
            };
            let instance = match instantiated {
                Ok(instance) => instance,
                Err(err) if err.is::<wasmtime::Trap>() => return Err(call_error(&mut ctx, err)),
                Err(err) => {
                    return Err(instantiation_error(ctx.data_mut().memory.explain(err)));
                }
            };
            let ty = component.inner.component_type();
            Exports::new(&store, &mut ctx, &instance, None, ty.exports(engine()))
        })?;
        Ok(Self {
            store,
            exports: exports.into_zval(false)?,
        })
    }

    #[php(getter)]
    pub fn get_exports(&self) -> Zval {
        self.exports.shallow_clone()
    }

    /// Hands `request` to the component's `wasi:http/incoming-handler` and
    /// returns its response.
    pub fn handle(&self, request: &Request) -> PhpResult<Response> {
        if self.store.is_parked() {
            return Err(store::busy());
        }
        let handler = self
            .func("wasi:http/incoming-handler", "handle")
            .ok_or_else(|| error("the component does not export wasi:http/incoming-handler"))?;
        http_handler::handle(&self.store, handler, request)
    }
}

impl Instance {
    pub fn store(&self) -> &SharedStore {
        &self.store
    }

    /// The function `name` of the exported interface `interface`, by name without version.
    pub fn func(&self, interface: &str, name: &str) -> Option<wasmtime::component::Func> {
        downcast::<Exports>(&self.exports)?
            .interface(interface)?
            .func(name)
    }
}
