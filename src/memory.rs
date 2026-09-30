use ext_php_rs::binary::Binary;
use ext_php_rs::binary_slice::BinarySlice;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::ZendHashTable;
use wasmtime::MemoryType;

use crate::error::{type_error, value_error};
use crate::store::{self, SharedStore};
use crate::value::descriptor_int;

/// Linear memory, like JS `WebAssembly.Memory`.
///
/// PHP has no shared ArrayBuffer, so reads and writes copy bytes in and out.
#[php_class]
#[php(name = "Wasm\\Memory")]
#[php(flags = ClassFlags::Final)]
pub struct Memory {
    pub store: SharedStore,
    pub inner: wasmtime::Memory,
}

#[php_impl]
impl Memory {
    /// @param array{initial: int, maximum?: int} $descriptor
    pub fn __construct(descriptor: &ZendHashTable) -> PhpResult<Self> {
        let initial = descriptor_int(descriptor, "initial")?
            .ok_or_else(|| type_error("descriptor \"initial\" is required"))?;
        let maximum = descriptor_int(descriptor, "maximum")?;
        let ty = MemoryType::new(page_count(initial)?, maximum.map(page_count).transpose()?);
        let store = store::current();
        let inner = store
            .with(|mut ctx| wasmtime::Memory::new(&mut ctx, ty))
            .map_err(|err| value_error(format!("{err:#}")))?;
        Ok(Self { store, inner })
    }

    /// Grows the memory by `delta` pages and returns the previous size in pages.
    pub fn grow(&self, delta: i64) -> PhpResult<i64> {
        let delta = u64::try_from(delta).map_err(|_| value_error("delta must not be negative"))?;
        self.store
            .with(|mut ctx| self.inner.grow(&mut ctx, delta))
            .map(|previous| previous as i64)
            .map_err(|err| value_error(format!("{err:#}")))
    }

    pub fn read(&self, offset: i64, length: i64) -> PhpResult<Binary<u8>> {
        let (offset, length) = (to_usize(offset, "offset")?, to_usize(length, "length")?);
        let mut buffer = vec![0; length];
        self.store
            .with(|ctx| self.inner.read(&ctx, offset, &mut buffer))
            .map_err(|_| {
                value_error(format!(
                    "reading {length} byte(s) at offset {offset} is out of bounds"
                ))
            })?;
        Ok(buffer.into())
    }

    pub fn write(&self, offset: i64, data: BinarySlice<u8>) -> PhpResult<()> {
        let offset = to_usize(offset, "offset")?;
        self.store
            .with(|mut ctx| self.inner.write(&mut ctx, offset, &data))
            .map_err(|_| {
                value_error(format!(
                    "writing {} byte(s) at offset {offset} is out of bounds",
                    data.len()
                ))
            })
    }

    pub fn byte_length(&self) -> i64 {
        self.store.with(|ctx| self.inner.data_size(&ctx) as i64)
    }

    /// A copy of the whole memory.
    pub fn buffer(&self) -> Binary<u8> {
        self.store.with(|ctx| self.inner.data(&ctx).to_vec()).into()
    }
}

fn page_count(pages: i64) -> PhpResult<u32> {
    u32::try_from(pages).map_err(|_| value_error(format!("{pages} pages is out of range")))
}

fn to_usize(value: i64, name: &str) -> PhpResult<usize> {
    usize::try_from(value).map_err(|_| value_error(format!("{name} must not be negative")))
}
