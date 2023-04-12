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
        let module = wasmer::Module::new(&store, &wat).expect("Failed parsing WAT!");
        let import_object = wasmer::imports! {};
        let instance = wasmer::Instance::new(&mut store, &module, &import_object).expect("Failed creating a wasm instance!");

        Ok(AddOneInstance {store, instance}.into())
    }

    pub fn add_one(&mut self, value: i32) -> PhpResult<i32> {
        let add_one = self.instance.exports.get_function("add_one").expect("Unable to locate the add_one function in the provided WAT!");
        let result = add_one.call(&mut self.store, &[wasmer::Value::I32(value)]).expect("Failed to execute add_ine method!");

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
