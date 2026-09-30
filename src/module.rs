use ext_php_rs::binary_slice::BinarySlice;
use ext_php_rs::boxed::ZBox;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::ZendHashTable;

use crate::engine::{compile_in_process_pool, engine};
use crate::error::compile_error;
use crate::imports::kind;

#[php_class]
#[php(name = "Wasm\\Module")]
#[php(flags = ClassFlags::Final)]
pub struct Module {
    pub inner: wasmtime::Module,
    /// wasmtime does not keep custom sections, so they are read once at compile time.
    custom_sections: Vec<(String, Vec<u8>)>,
}

#[php_impl]
impl Module {
    /// Compiles a wasm binary or WAT text.
    pub fn __construct(bytes: BinarySlice<u8>) -> PhpResult<Self> {
        Self::compile(&bytes)
    }

    /// @return list<array{name: string, kind: string}>
    pub fn exports(module: &Module) -> PhpResult<ZBox<ZendHashTable>> {
        let mut list = ZendHashTable::new();
        for export in module.inner.exports() {
            let mut entry = ZendHashTable::new();
            entry.insert("name", export.name())?;
            entry.insert("kind", kind(&export.ty()))?;
            list.push(entry)?;
        }
        Ok(list)
    }

    /// @return list<array{module: string, name: string, kind: string}>
    pub fn imports(module: &Module) -> PhpResult<ZBox<ZendHashTable>> {
        let mut list = ZendHashTable::new();
        for import in module.inner.imports() {
            let mut entry = ZendHashTable::new();
            entry.insert("module", import.module())?;
            entry.insert("name", import.name())?;
            entry.insert("kind", kind(&import.ty()))?;
            list.push(entry)?;
        }
        Ok(list)
    }

    /// @return list<string>
    pub fn custom_sections(module: &Module, name: String) -> PhpResult<ZBox<ZendHashTable>> {
        let mut list = ZendHashTable::new();
        for (_, data) in module
            .custom_sections
            .iter()
            .filter(|(section, _)| *section == name)
        {
            list.push(ext_php_rs::binary::Binary::from(data.clone()))?;
        }
        Ok(list)
    }
}

impl Module {
    pub fn compile(bytes: &[u8]) -> PhpResult<Self> {
        let binary = wat::parse_bytes(bytes).map_err(compile_error)?;
        let inner = compile_in_process_pool(|| wasmtime::Module::from_binary(engine(), &binary))
            .map_err(compile_error)?
            .map_err(compile_error)?;
        Ok(Self {
            inner,
            custom_sections: custom_sections(&binary),
        })
    }
}

pub fn validate(bytes: &[u8]) -> bool {
    wat::parse_bytes(bytes).is_ok_and(|binary| {
        compile_in_process_pool(|| wasmtime::Module::validate(engine(), &binary))
            .is_ok_and(|valid| valid.is_ok())
    })
}

fn custom_sections(binary: &[u8]) -> Vec<(String, Vec<u8>)> {
    wasmparser::Parser::new(0)
        .parse_all(binary)
        .filter_map(|payload| match payload {
            Ok(wasmparser::Payload::CustomSection(section)) => {
                Some((section.name().to_string(), section.data().to_vec()))
            }
            _ => None,
        })
        .collect()
}
