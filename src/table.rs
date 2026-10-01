use crate::limits::limited;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::{HeapTopType, RefType, TableType};

use crate::error::{type_error, value_error};
use crate::func::Func;
use crate::store::{self, SharedStore, StoreObject};
use crate::value::{descriptor_int, descriptor_str, downcast, from_ref, to_ref};

/// A table of references, like JS `WebAssembly.Table`.
#[php_class]
#[php(name = "Wasm\\Table")]
#[php(flags = ClassFlags::Final)]
pub struct Table {
    pub store: SharedStore,
    pub inner: wasmtime::Table,
}

#[php_impl]
impl Table {
    /// @param array{element: 'anyfunc'|'externref', initial: int, maximum?: int} $descriptor
    pub fn __construct(
        descriptor: &ZendHashTable,
        value: Option<&Zval>,
        store: Option<&StoreObject>,
    ) -> PhpResult<Self> {
        let element = match descriptor_str(descriptor, "element")? {
            Some("anyfunc" | "funcref") => RefType::FUNCREF,
            Some("externref") => RefType::EXTERNREF,
            Some(other) => {
                return Err(type_error(format!(
                    "unknown table element type \"{other}\""
                )));
            }
            None => return Err(type_error("descriptor \"element\" is required")),
        };
        let initial = descriptor_int(descriptor, "initial")?
            .ok_or_else(|| type_error("descriptor \"initial\" is required"))?;
        let maximum = descriptor_int(descriptor, "maximum")?;
        let ty = TableType::new(
            element.clone(),
            to_u32(initial)?,
            maximum.map(to_u32).transpose()?,
        );
        let null = Zval::null();
        // An externref value is a plain PHP value, even when it is a wasm object.
        let from = value
            .filter(|_| element.heap_type().top() == HeapTopType::Func)
            .and_then(downcast::<Func>)
            .map(|func| (func.store.clone(), "Func"));
        let store = store::choose(store, from, store::standalone)?;
        let inner = store.with(|mut ctx| {
            let init = to_ref(&mut ctx, value.unwrap_or(&null), &element)?;
            limited(&mut ctx, |ctx| wasmtime::Table::new(ctx, ty, init))
                .map_err(|err| value_error(format!("{err:#}")))
        })?;
        Ok(Self { store, inner })
    }

    pub fn get(&self, index: i64) -> PhpResult<Zval> {
        let index = to_index(index)?;
        self.store.with(|mut ctx| {
            let value = self
                .inner
                .get(&mut ctx, index)
                .ok_or_else(|| value_error(format!("table index {index} is out of bounds")))?;
            Ok(from_ref(&mut ctx, &value)?)
        })
    }

    pub fn set(&self, index: i64, value: Option<&Zval>) -> PhpResult<()> {
        let index = to_index(index)?;
        let null = Zval::null();
        self.store.with(|mut ctx| {
            let element = self.inner.ty(&ctx).element().clone();
            let value = to_ref(&mut ctx, value.unwrap_or(&null), &element)?;
            self.inner
                .set(&mut ctx, index, value)
                .map_err(|_| value_error(format!("table index {index} is out of bounds")))
        })
    }

    /// Grows the table by `delta` entries and returns the previous length.
    pub fn grow(&self, delta: i64, value: Option<&Zval>) -> PhpResult<i64> {
        let delta = u64::try_from(delta).map_err(|_| value_error("delta must not be negative"))?;
        let null = Zval::null();
        self.store.with(|mut ctx| {
            let element = self.inner.ty(&ctx).element().clone();
            let init = to_ref(&mut ctx, value.unwrap_or(&null), &element)?;
            limited(&mut ctx, |ctx| self.inner.grow(ctx, delta, init))
                .map(|previous| previous as i64)
                .map_err(|err| value_error(format!("{err:#}")))
        })
    }

    pub fn length(&self) -> i64 {
        self.store.with(|ctx| self.inner.size(&ctx) as i64)
    }
}

fn to_u32(value: i64) -> PhpResult<u32> {
    u32::try_from(value)
        .map_err(|_| value_error(format!("{value} is out of range for a table size")))
}

fn to_index(index: i64) -> PhpResult<u64> {
    u64::try_from(index).map_err(|_| value_error("table index must not be negative"))
}
