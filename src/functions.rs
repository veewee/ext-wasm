use ext_php_rs::binary_slice::BinarySlice;
use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};

use crate::error::type_error;
use crate::instance::Instance;
use crate::module::Module;
use crate::value::{debug_type, downcast};

/// Whether `bytes` is a valid wasm binary or WAT module.
#[php_function]
#[php(name = "Wasm\\validate")]
pub fn validate(bytes: BinarySlice<u8>) -> bool {
    crate::module::validate(&bytes)
}

#[php_function]
#[php(name = "Wasm\\compile")]
pub fn compile(bytes: BinarySlice<u8>) -> PhpResult<Module> {
    Module::compile(&bytes)
}

/// Like JS `WebAssembly.instantiate()`: bytes give `['module' => Module, 'instance' => Instance]`,
/// a Module gives the Instance.
///
/// @return Instance|array{module: Module, instance: Instance}
#[php_function]
#[php(name = "Wasm\\instantiate")]
pub fn instantiate(source: &Zval, imports: Option<&ZendHashTable>) -> PhpResult<Zval> {
    if let Some(module) = downcast::<Module>(source) {
        return Ok(Instance::__construct(module, imports)?.into_zval(false)?);
    }
    let bytes = source.zend_str().ok_or_else(|| {
        type_error(format!(
            "expected string or Wasm\\Module, got {}",
            debug_type(source)
        ))
    })?;
    let module = Module::compile(bytes.as_bytes())?;
    let instance = Instance::__construct(&module, imports)?;
    let mut result = ZendHashTable::new();
    result.insert("module", module.into_zval(false)?)?;
    result.insert("instance", instance.into_zval(false)?)?;
    Ok(result.into_zval(false)?)
}

pub fn register(module: ModuleBuilder) -> ModuleBuilder {
    module
        .function(wrap_function!(validate))
        .function(wrap_function!(compile))
        .function(wrap_function!(instantiate))
}
