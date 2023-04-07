use ext_php_rs::prelude::ModuleBuilder;

pub mod engines;

pub fn register(mut module: ModuleBuilder) -> ModuleBuilder {
    module = engines::register(module);

    module
}

pub fn build() {
    
}
