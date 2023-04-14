use ext_php_rs::prelude::*;
use ext_php_rs::zend::ModuleEntry;
use ext_php_rs::*;

#[php_class(name="Wasm\\WasmInstance")]
pub struct WasmInstance {
    pub store: wasmer::Store,
    pub instance: wasmer::Instance,
}

#[php_impl]
impl WasmInstance {
    pub fn __construct(wat : String) -> PhpResult<WasmInstance> {
        let mut store = wasmer::Store::default();
        let module = wasmer::Module::new(&store, &wat)
            .map_err(|err| PhpException::default(err.to_string()))?;
        
        let import_object = wasmer::imports! {};
        let instance = wasmer::Instance::new(&mut store, &module, &import_object)
            .map_err(|err| PhpException::default(err.to_string()))?;

        Ok(WasmInstance {store, instance}.into())
    }

    pub fn __call(&mut self, method: String, attributes: Vec<i32>) -> PhpResult<Option<i32>> {
        let func = self.instance.exports.get_function(&method)
            .map_err(|err| PhpException::default(err.to_string()))?;
            
        let wasm_attributes : Vec<wasmer::Value> = attributes.into_iter()
            .map(|value| wasmer::Value::I32(value))
            .collect();

        let result = func.call(&mut self.store, &wasm_attributes)
            .map(<[_]>::into_vec)
            .map_err(|err| PhpException::default(err.to_string()))?;

        Ok(match result.len() {
            0 => None,
            1 => Some(result[0].unwrap_i32()),
            _ => None // TODO: return tupple
        })
    }

    pub fn __get(&mut self, accessor: String) -> PhpResult<i32> {
        let prop = self.instance.exports.get_global(&accessor)
            .map_err(|err| PhpException::default(err.to_string()))?;

        let value = prop.get(&mut self.store);

        Ok(value.unwrap_i32())
    }

    pub fn __set(&mut self, accessor: String, value: i32) -> PhpResult<Option<()>> {
        let prop = self.instance.exports.get_global(&accessor)
            .map_err(|err| PhpException::default(err.to_string()))?;

        prop.set(&mut self.store, wasmer::Value::I32(value))
            .map_err(|err| PhpException::default(err.to_string()))?;

        Ok(None)
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
