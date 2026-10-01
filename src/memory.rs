use crate::limits::limited;
use ext_php_rs::binary::Binary;
use ext_php_rs::binary_slice::BinarySlice;
use ext_php_rs::boxed::ZBox;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::ZendHashTable;
use wasmtime::MemoryType;

use crate::error::value_error;
use crate::store::{self, SharedStore, StoreObject};
use crate::types::memory_type;
use crate::value::{descriptor_address, descriptor_int, descriptor_minimum};

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
    /// @param array{initial?: int, minimum?: int, maximum?: int, address?: 'i32'|'i64'} $descriptor
    pub fn __construct(descriptor: &ZendHashTable, store: Option<&StoreObject>) -> PhpResult<Self> {
        let initial = descriptor_minimum(descriptor)?;
        let maximum = descriptor_int(descriptor, "maximum")?;
        // The builder reports what MemoryType::new would panic on, such as a
        // minimum above the maximum.
        let ty = MemoryType::builder()
            .memory64(descriptor_address(descriptor)?)
            .min(initial as u64)
            .max(maximum.map(|n| n as u64))
            .build()
            .map_err(|err| value_error(format!("{err:#}")))?;
        let store = store::choose(store, [], store::standalone)?;
        let inner = store
            .with(|mut ctx| limited(&mut ctx, |ctx| wasmtime::Memory::new(ctx, ty)))
            .map_err(|err| value_error(format!("{err:#}")))?;
        Ok(Self { store, inner })
    }

    /// Grows the memory by `delta` pages and returns the previous size in pages.
    pub fn grow(&self, delta: i64) -> PhpResult<i64> {
        let delta = u64::try_from(delta).map_err(|_| value_error("delta must not be negative"))?;
        self.store
            .with(|mut ctx| limited(&mut ctx, |ctx| self.inner.grow(ctx, delta)))
            .map(|previous| previous as i64)
            .map_err(|err| value_error(format!("{err:#}")))
    }

    pub fn read(&self, offset: i64, length: i64) -> PhpResult<Binary<u8>> {
        let (offset, length) = (to_usize(offset, "offset")?, to_usize(length, "length")?);
        // Checked before copying, so a bogus length throws instead of allocating.
        self.store.with(|ctx| {
            let data = self.inner.data(&ctx);
            offset
                .checked_add(length)
                .and_then(|end| data.get(offset..end))
                .map(|bytes| bytes.to_vec().into())
                .ok_or_else(|| {
                    value_error(format!(
                        "reading {length} byte(s) at offset {offset} is out of bounds"
                    ))
                })
        })
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

    /// The memory's type, with its current size in pages as `minimum`.
    ///
    /// @return array{minimum: int, maximum?: int, address?: 'i64'}
    pub fn r#type(&self) -> PhpResult<ZBox<ZendHashTable>> {
        self.store
            .with(|ctx| memory_type(&self.inner.ty(&ctx), self.inner.size(&ctx)))
    }

    pub fn byte_length(&self) -> i64 {
        self.store.with(|ctx| self.inner.data_size(&ctx) as i64)
    }

    /// A copy of the whole memory.
    pub fn buffer(&self) -> Binary<u8> {
        self.store.with(|ctx| self.inner.data(&ctx).to_vec()).into()
    }
}

fn to_usize(value: i64, name: &str) -> PhpResult<usize> {
    usize::try_from(value).map_err(|_| value_error(format!("{name} must not be negative")))
}
