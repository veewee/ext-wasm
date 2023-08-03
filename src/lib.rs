#![cfg_attr(windows, feature(abi_vectorcall))]

mod types;

use std::collections::HashMap;
use ext_php_rs::prelude::*;
use ext_php_rs::zend::ModuleEntry;
use ext_php_rs::types::{ZendClassObject};
use ext_php_rs::*;
use crate::types::value::Value;

#[php_class(name="Wasm\\WasmInstance")]
pub struct WasmInstance {
    pub store: wasmer::Store,
    pub instance: wasmer::Instance,
}

#[php_impl]
impl WasmInstance {
    pub fn from_builder(builder : &mut ZendClassObject<InstanceBuilder>) -> PhpResult<WasmInstance> {
        builder.build()
    }

    pub fn __call(&mut self, method: String, attributes: Vec<Value>) -> PhpResult<Option<Vec<Value>>> {
        let func = self.instance.exports.get_function(&method)
            .map_err(|err| PhpException::default(err.to_string()))?;
            
        let wasm_attributes : Vec<wasmer::Value> = attributes.into_iter()
            .map(|value| value.into())
            .collect();

        let result : Vec<Value> = func.call(&mut self.store, &wasm_attributes)
            .map(<[_]>::into_vec)
            .map(|vec| vec.into_iter().map(|value| Value::from(&value)).collect())
            .map_err(|err| PhpException::default(err.to_string()))?;

        Ok(match result.len() {
            0 => None,
            _ => Some(result),
        })
    }

    pub fn __get(&mut self, accessor: String) -> PhpResult<Value> {
        let prop = self.instance.exports.get_global(&accessor)
            .map_err(|err| PhpException::default(err.to_string()))?;

        let value = prop.get(&mut self.store);

        Ok(Value::from(&value))
    }

    pub fn __set(&mut self, accessor: String, value: Value) -> PhpResult<Option<()>> {
        let prop = self.instance.exports.get_global(&accessor)
            .map_err(|err| PhpException::default(err.to_string()))?;

        prop.set(&mut self.store, value.into())
            .map_err(|err| PhpException::default(err.to_string()))?;

        Ok(None)
    }
}

#[php_class(name="Wasm\\InstanceBuilder")]
#[derive(Default)]
pub struct InstanceBuilder {
    pub wat: Box<String>,
    pub imports : Option<Box<WasmImports>>,
}

#[php_impl]
impl InstanceBuilder {    
    pub fn from_wat(wat: String) -> InstanceBuilder {
        InstanceBuilder {
            wat: wat.clone().into(),
            ..Default::default()
        }.into()
    }

    // TODO : Change return type to be fluent (see #28)
    pub fn import<'a>(
        #[this] this: &mut ZendClassObject<InstanceBuilder>,
        imports: &mut ZendClassObject<WasmImports>
    ) -> () {
        this.imports = Some((*imports).clone().into());
    }

    pub fn build(&mut self) -> PhpResult<WasmInstance> {
        let mut store = wasmer::Store::default();
        let module = wasmer::Module::new(&store, &*self.wat)
            .map_err(|err| PhpException::default(err.to_string()))?;

        // Build imports
        let mut import_object = wasmer::Imports::new();
        if self.imports.is_some() {
            import_object = (self.imports.as_mut().unwrap()).into_wasmer_imports(&mut store)
        }
        
        let instance = wasmer::Instance::new(&mut store, &module, &import_object)
            .map_err(|err| PhpException::default(err.to_string()))?;

        Ok(WasmInstance {store, instance}.into())
    }
}


#[php_class(name="Wasm\\Imports")]
#[derive(Clone)]
pub struct WasmImports {
    pub registry : HashMap<(String, String), WasmGlobal>,
}

#[php_impl]
impl WasmImports {    
    pub fn create() -> WasmImports {
        WasmImports {
            registry: HashMap::new().into()
        }.into()
    }

    // TODO : Change return type to be fluent (see #28)
    pub fn define(
        #[this] this: &mut ZendClassObject<WasmImports>,
        namespace : String,
        variable : String,
        value : &mut ZendClassObject<WasmGlobal>,
    ) -> () {
        this.registry.insert((namespace.clone(), variable.clone()), (*value).clone().into());
    }
}

impl WasmImports {
    pub fn into_wasmer_imports(&mut self, store : &mut wasmer::Store) -> wasmer::Imports {
        let mut import_object = wasmer::Imports::new();

        for ((namespace, name), value) in (&self.registry).into_iter() {
            let converted = value.clone().into_wasmer_global(store);
            import_object.define(&namespace, &name, converted);
        }

        import_object
    }
}

#[php_class(name="Wasm\\Type\\Global")]
#[derive(Clone)]
pub struct WasmGlobal {
    pub value : Box<Value>,
    pub mutable : bool,
}

#[php_impl]
impl WasmGlobal {
    pub fn mutable(value: Value) -> WasmGlobal {
        WasmGlobal {value: value.into(), mutable: true}.into()
    }

    pub fn immutable(value: Value) -> WasmGlobal {
        WasmGlobal {value: value.into(), mutable: false}.into()
    }
}

impl WasmGlobal {
    pub fn into_wasmer_global(&mut self, store : &mut wasmer::Store) -> wasmer::Global {
        match self.mutable {
            true => wasmer::Global::new_mut(store, (*self.value).into()),
            false => wasmer::Global::new(store, (*self.value).into()),
        }
    }

    pub fn into_wasmer_extern(&mut self, store : &mut wasmer::Store) -> wasmer::Extern {
        wasmer::Extern::Global(
            self.into_wasmer_global(store)
        )
    }
}

/// Used by the `phpinfo()` function and when you run `php -i`.
pub extern "C" fn php_module_info(_module: *mut ModuleEntry) {
    info_table_start!();
    info_table_row!("Wasm", "enabled");
    info_table_end!();
}

#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    module
        .info_function(php_module_info)
}
