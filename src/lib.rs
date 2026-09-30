#![cfg_attr(windows, feature(abi_vectorcall))]

mod engine;
mod error;
mod exports;
mod func;
mod instance;
mod module;
mod store;
mod value;

use ext_php_rs::prelude::*;
use ext_php_rs::zend::ModuleEntry;
use ext_php_rs::{info_table_end, info_table_row, info_table_start};

/// Used by `phpinfo()` and `php -i`.
pub extern "C" fn php_module_info(_module: *mut ModuleEntry) {
    info_table_start!();
    info_table_row!("wasm support", "enabled");
    info_table_row!("wasm runtime", "wasmtime 49.0.1");
    info_table_end!();
}

#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    module
        .info_function(php_module_info)
        .class::<error::WasmException>()
        .class::<error::CompileError>()
        .class::<error::LinkError>()
        .class::<error::RuntimeError>()
        .class::<module::Module>()
        .class::<func::Func>()
        .class::<exports::Exports>()
        .class::<instance::Instance>()
}
