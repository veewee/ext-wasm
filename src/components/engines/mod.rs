use ext_php_rs::prelude::ModuleBuilder;

pub mod interfaces;
pub mod implementations;


pub fn register(module: ModuleBuilder) -> ModuleBuilder {
    interfaces::build();
    implementations::build();

    module
}
