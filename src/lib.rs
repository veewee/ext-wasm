#![cfg_attr(windows, feature(abi_vectorcall))]
use ext_php_rs::prelude::*;
use ext_php_rs::zend::ModuleEntry;
use ext_php_rs::*;


#[php_class(name="Wasm\\Store\\DefaultStore")]
#[derive(Default)]
pub struct DefaultStore {
    inner: wasmer::Store,
}

#[php_impl]
impl DefaultStore {
    pub fn __construct() -> DefaultStore {
        DefaultStore {
            inner: wasmer::Store::default()
        }.into()
    }
}


#[php_startup]
pub fn setup() {
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
