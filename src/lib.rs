#![cfg_attr(windows, feature(abi_vectorcall))]
use ext_php_rs::prelude::*;
use ext_php_rs::zend::ModuleEntry;
use ext_php_rs::*;


pub mod macros;
pub mod components;
pub mod exceptions;


#[php_startup]
pub fn setup() {
    exceptions::build();
    components::build();
}

/// Used by the `phpinfo()` function and when you run `php -i`.
pub extern "C" fn php_module_info(_module: *mut ModuleEntry) {
    info_table_start!();
    info_table_row!("Wasm", "enabled");
    info_table_end!();
}

#[php_module]
pub fn get_module(mut module: ModuleBuilder) -> ModuleBuilder {
    module = module.info_function(php_module_info);
    module = components::register(module);

    module
}
