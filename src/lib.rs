#![cfg_attr(windows, feature(abi_vectorcall))]

mod types;

use ext_php_rs::prelude::*;
use ext_php_rs::zend::ModuleEntry;
use ext_php_rs::types::ZendClassObject;
use ext_php_rs::*;
use crate::types::Value;

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

    pub fn __call(&mut self, method: String, attributes: Vec<Value>) -> PhpResult<Option<Value>> {
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
            1 => Some(result[0]),
            _ => None // Todo support: Some(result), "Copy" solution for Value might not be best for case "1" either
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
pub struct InstanceBuilder {
    pub wat: Box<String>,
}

#[php_impl]
impl InstanceBuilder {    
    pub fn from_wat(wat: String) -> InstanceBuilder {
        InstanceBuilder {
            wat: wat.clone().into()
        }.into()
    }

    pub fn build(&mut self) -> PhpResult<WasmInstance> {
        let mut store = wasmer::Store::default();
        let module = wasmer::Module::new(&store, &*self.wat)
            .map_err(|err| PhpException::default(err.to_string()))?;
        
        let import_object = wasmer::imports! {};
        let instance = wasmer::Instance::new(&mut store, &module, &import_object)
            .map_err(|err| PhpException::default(err.to_string()))?;

        Ok(WasmInstance {store, instance}.into())
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
