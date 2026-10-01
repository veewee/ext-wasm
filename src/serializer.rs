use ext_php_rs::binary::Binary;
use ext_php_rs::binary_slice::BinarySlice;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;

use crate::component::Component;
use crate::engine::engine;
use crate::error::compile_error;
use crate::module::{Module, read_local_file};
use crate::precompiled::{self, Kind};

/// Turns compiled modules and components into precompiled artifacts and back,
/// so production can load them without compiling.
///
/// An artifact is native machine code. Only load artifacts you built yourself
/// and stored where nobody else can write: a crafted artifact can run any code
/// with the rights of PHP, like loading an extension. The checksum in an artifact
/// catches corruption, not tampering.
///
/// An artifact loads only on a host with the same OS and CPU architecture, a
/// CPU with at least the features of the one that built it, the same wasmtime
/// major version and ext-wasm engine settings that wasmtime accepts as
/// compatible; anything else is a CompileError.
#[php_class]
#[php(name = "Wasm\\Serializer")]
#[php(flags = ClassFlags::Final)]
pub struct Serializer;

#[php_impl]
impl Serializer {
    pub fn __construct() -> Self {
        Self
    }

    pub fn serialize_module(&self, module: &Module) -> PhpResult<Binary<u8>> {
        let payload = module.inner.serialize().map_err(compile_error)?;
        Ok(precompiled::encode(Kind::Module, module.sections(), payload).into())
    }

    pub fn serialize_component(&self, component: &Component) -> PhpResult<Binary<u8>> {
        let payload = component.inner.serialize().map_err(compile_error)?;
        Ok(precompiled::encode(Kind::Component, &[], payload).into())
    }

    pub fn deserialize_module(&self, artifact: BinarySlice<u8>) -> PhpResult<Module> {
        load_module(&artifact)
    }

    pub fn deserialize_component(&self, artifact: BinarySlice<u8>) -> PhpResult<Component> {
        load_component(&artifact)
    }

    /// Reads local files only and honours open_basedir.
    pub fn deserialize_module_file(&self, path: String) -> PhpResult<Module> {
        load_module(&read_local_file(&path)?)
    }

    /// Reads local files only and honours open_basedir.
    pub fn deserialize_component_file(&self, path: String) -> PhpResult<Component> {
        load_component(&read_local_file(&path)?)
    }
}

fn load_module(bytes: &[u8]) -> PhpResult<Module> {
    let artifact = precompiled::decode(bytes, Kind::Module).map_err(compile_error)?;
    // SAFETY: wasmtime requires the payload to be its own serialize output. Nothing
    // here can check that: the checksum only catches corruption, and wasmtime
    // only checks engine compatibility. The class docblock makes it the caller's
    // guarantee, the same trust as loading an extension.
    let inner = unsafe { wasmtime::Module::deserialize(engine(), artifact.payload) }
        .map_err(|err| compile_error(precompiled::incompatible(err)))?;
    Ok(Module::from_parts(inner, artifact.sections))
}

fn load_component(bytes: &[u8]) -> PhpResult<Component> {
    let artifact = precompiled::decode(bytes, Kind::Component).map_err(compile_error)?;
    // SAFETY: as in load_module.
    let inner = unsafe { wasmtime::component::Component::deserialize(engine(), artifact.payload) }
        .map_err(|err| compile_error(precompiled::incompatible(err)))?;
    Ok(Component { inner })
}
