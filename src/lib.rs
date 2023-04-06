#![cfg_attr(windows, feature(abi_vectorcall))]
use ext_php_rs::prelude::*;

#[php_function]
pub fn hello_world(name: &str) -> String {
    format!("Hello, {}!", name)
}

#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    module
}
