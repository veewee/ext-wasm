use std::ffi::{CStr, CString};

use ext_php_rs::alloc::efree;
use ext_php_rs::binary_slice::BinarySlice;
use ext_php_rs::boxed::ZBox;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::ZendHashTable;

use crate::engine::{compile_in_process_pool, engine};
use crate::error::{compile_error, wasm_exception};
use crate::imports::kind;
use crate::types::extern_type;

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

    /// Compiles a wasm binary or WAT file, like `new Module(file_get_contents($path))`.
    ///
    /// Reads local files only and honours open_basedir. Use file_get_contents()
    /// for stream wrappers such as phar:// or compress.zlib://.
    pub fn from_file(path: String) -> PhpResult<Self> {
        Self::compile(&read_local_file(&path)?)
    }

    /// Each entry's `type` is shaped like the JS type reflection proposal:
    /// `{parameters, results}` for a function, `{value, mutable}` for a global,
    /// `{minimum, maximum?}` for a memory, `{element, minimum, maximum?}` for a
    /// table and `{parameters}` for a tag.
    ///
    /// @return list<array{name: string, kind: string, type: array<string, mixed>}>
    pub fn exports(&self) -> PhpResult<ZBox<ZendHashTable>> {
        let mut list = ZendHashTable::new();
        for export in self.inner.exports() {
            let mut entry = ZendHashTable::new();
            entry.insert("name", export.name())?;
            entry.insert("kind", kind(&export.ty()))?;
            entry.insert("type", extern_type(&export.ty())?)?;
            list.push(entry)?;
        }
        Ok(list)
    }

    /// `type` is shaped as in exports().
    ///
    /// @return list<array{module: string, name: string, kind: string, type: array<string, mixed>}>
    pub fn imports(&self) -> PhpResult<ZBox<ZendHashTable>> {
        let mut list = ZendHashTable::new();
        for import in self.inner.imports() {
            let mut entry = ZendHashTable::new();
            entry.insert("module", import.module())?;
            entry.insert("name", import.name())?;
            entry.insert("kind", kind(&import.ty()))?;
            entry.insert("type", extern_type(&import.ty())?)?;
            list.push(entry)?;
        }
        Ok(list)
    }

    /// @return list<string>
    pub fn custom_sections(&self, name: String) -> PhpResult<ZBox<ZendHashTable>> {
        let mut list = ZendHashTable::new();
        for (_, data) in self
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
    pub fn from_parts(inner: wasmtime::Module, custom_sections: Vec<(String, Vec<u8>)>) -> Self {
        Self {
            inner,
            custom_sections,
        }
    }

    pub fn sections(&self) -> &[(String, Vec<u8>)] {
        &self.custom_sections
    }

    pub fn compile(bytes: &[u8]) -> PhpResult<Self> {
        let binary = wat::parse_bytes(bytes).map_err(compile_error)?;
        let inner = compile_in_process_pool(|| wasmtime::Module::from_binary(engine(), &binary))
            .map_err(compile_error)?
            .map_err(|err| {
                if wasmparser::Parser::is_component(&binary) {
                    compile_error(format!(
                        "{err:#} (this is a component, use Wasm\\Component\\Component)"
                    ))
                } else {
                    compile_error(err)
                }
            })?;
        Ok(Self {
            inner,
            custom_sections: custom_sections(&binary),
        })
    }
}

unsafe extern "C" {
    fn expand_filepath(
        filepath: *const std::ffi::c_char,
        real_path: *mut std::ffi::c_char,
    ) -> *mut std::ffi::c_char;
    fn php_check_open_basedir_ex(
        path: *const std::ffi::c_char,
        warn: std::ffi::c_int,
    ) -> std::ffi::c_int;
}

/// Reads a file the way PHP's own file functions find it: relative to PHP's
/// working directory, which differs from the process one in ZTS builds, and
/// only inside open_basedir.
pub(crate) fn read_local_file(path: &str) -> PhpResult<Vec<u8>> {
    let fail = |reason: &str| wasm_exception(format!("cannot read {path}: {reason}"));
    let c_path = CString::new(path).map_err(|_| fail("the path contains a NUL byte"))?;
    // SAFETY: `c_path` is a valid C string; PHP returns an emalloc'd copy or null.
    let expanded = unsafe { expand_filepath(c_path.as_ptr(), std::ptr::null_mut()) };
    if expanded.is_null() {
        return Err(fail("the path cannot be resolved"));
    }
    // SAFETY: `expanded` is a C string PHP allocated for us, freed right after the copy.
    let absolute = unsafe { CStr::from_ptr(expanded) }.to_owned();
    unsafe { efree(expanded.cast()) };
    // SAFETY: `absolute` is a valid C string; 0 suppresses PHP's own warning.
    if unsafe { php_check_open_basedir_ex(absolute.as_ptr(), 0) } != 0 {
        return Err(fail("the path is outside open_basedir"));
    }
    let absolute = absolute
        .to_str()
        .map_err(|_| fail("the path is not valid UTF-8"))?;
    std::fs::read(absolute).map_err(|err| fail(&err.to_string()))
}

/// Whether `bytes` is a valid module or component, binary or WAT.
pub fn validate(bytes: &[u8]) -> bool {
    wat::parse_bytes(bytes).is_ok_and(|binary| {
        crate::component::validate(&binary)
            || compile_in_process_pool(|| wasmtime::Module::validate(engine(), &binary))
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
