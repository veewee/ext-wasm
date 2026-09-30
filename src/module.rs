use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::binary_slice::BinarySlice;

use crate::engine::engine;
use crate::error::compile_error;

#[php_class]
#[php(name = "Wasm\\Module")]
#[php(flags = ClassFlags::Final)]
pub struct Module {
    pub inner: wasmtime::Module,
}

#[php_impl]
impl Module {
    /// Compiles a wasm binary or WAT text.
    pub fn __construct(bytes: BinarySlice<u8>) -> PhpResult<Self> {
        Ok(Self { inner: compile(&bytes)? })
    }
}

pub fn compile(bytes: &[u8]) -> PhpResult<wasmtime::Module> {
    wasmtime::Module::new(engine(), bytes).map_err(compile_error)
}
