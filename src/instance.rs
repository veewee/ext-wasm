use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::Extern;

use crate::error::instantiation_error;
use crate::exports::Exports;
use crate::imports;
use crate::module::Module;
use crate::store::{self, StoreObject};
use crate::suspend;
use crate::throw::call_error;
use crate::value::downcast;

#[php_class]
#[php(name = "Wasm\\Instance")]
#[php(flags = ClassFlags::Final)]
pub struct Instance {
    exports: Zval,
}

#[php_impl]
impl Instance {
    pub fn __construct(
        module: &Module,
        imports: Option<&ZendHashTable>,
        store: Option<&StoreObject>,
    ) -> PhpResult<Self> {
        let store = store::choose(store, imports::stores(&module.inner, imports), store::new)?;
        if store.is_parked() {
            return Err(store::busy());
        }
        if imports::has_suspending(&module.inner, imports) {
            store.make_async()?;
        }
        store::retire_standalone(&store);
        let imports = imports::resolve(&store, &module.inner, imports)?;
        let externs: Vec<(String, Extern)> = store.with(|mut ctx| {
            ctx.data_mut().memory.reset();
            let created = if ctx.data().is_async {
                suspend::drive(
                    &store,
                    wasmtime::Instance::new_async(&mut ctx, &module.inner, &imports),
                )
            } else {
                wasmtime::Instance::new(&mut ctx, &module.inner, &imports)
            };
            let instance = match created {
                Ok(instance) => instance,
                Err(err) if err.is::<wasmtime::Trap>() || err.is::<wasmtime::ThrownException>() => {
                    return Err(call_error(&mut ctx, err));
                }
                Err(err) => {
                    // A module that failed to link frees the memories it created.
                    let memory = &mut ctx.data_mut().memory;
                    memory.undo();
                    return Err(instantiation_error(memory.explain(err)));
                }
            };
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

impl Instance {
    pub fn exports_object(&self) -> &Exports {
        downcast::<Exports>(&self.exports).expect("an instance always holds its Exports")
    }
}
