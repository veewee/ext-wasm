#![cfg_attr(windows, feature(abi_vectorcall))]

use ext_php_rs::prelude::*;
use ext_php_rs::zend::ModuleEntry;
use ext_php_rs::*;

#[php_class(name="Wasm\\AddOneInstance")]
pub struct AddOneInstance {
    pub store: wasmer::Store,
    pub instance: wasmer::Instance,
}

#[php_impl]
impl AddOneInstance {
    pub fn __construct(wat : String) -> PhpResult<AddOneInstance> {
        let mut store = wasmer::Store::default();
        let module = match wasmer::Module::new(&store, &wat) {
            Err(e) => return Err(PhpException::default(e.to_string())),
            Ok(f) => f,
        };
        let import_object = wasmer::imports! {};
        let instance = match wasmer::Instance::new(&mut store, &module, &import_object) {
            Err(e) => return Err(PhpException::default(e.to_string())),
            Ok(f) => f,
        };

        Ok(AddOneInstance {store, instance}.into())
    }

    pub fn add_one(&mut self, value: i32) -> PhpResult<i32> {
        let add_one = match self.instance.exports.get_function("add_one") {
            Err(e) => return Err(PhpException::default(e.to_string())),
            Ok(f) => f,
        };
        let result = match add_one.call(&mut self.store, &[wasmer::Value::I32(value)]) {
            Err(e) => return Err(PhpException::default(e.to_string())),
            Ok(f) => f,
        };

        Ok(result[0].unwrap_i32())
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
