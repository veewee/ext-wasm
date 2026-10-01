use crate::limits::limited;
use ext_php_rs::boxed::ZBox;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::{HeapTopType, TableType, ValType};

use crate::error::{type_error, value_error};
use crate::func::Func;
use crate::store::{self, SharedStore, StoreObject};
use crate::types::table_type;
use crate::value::{
    descriptor_address, descriptor_int, descriptor_minimum, descriptor_str, downcast, from_ref,
    parse_val_type, to_ref,
};

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
    /// `element` is `funcref`, `externref`, `nullfuncref`, `nullexternref`,
    /// `(ref func)` or `(ref extern)`; the last two need a `$value`.
    ///
    /// @param array{element: string, initial?: int, minimum?: int, maximum?: int, address?: 'i32'|'i64'} $descriptor
    pub fn __construct(
        descriptor: &ZendHashTable,
        value: Option<&Zval>,
        store: Option<&StoreObject>,
    ) -> PhpResult<Self> {
        let name = descriptor_str(descriptor, "element")?
            .ok_or_else(|| type_error("descriptor \"element\" is required"))?;
        let ValType::Ref(element) = parse_val_type(name)? else {
            return Err(type_error(format!(
                "table element type \"{name}\" is not a reference type"
            )));
        };
        let initial = descriptor_minimum(descriptor)?;
        let maximum = descriptor_int(descriptor, "maximum")?;
        if let Some(maximum) = maximum.filter(|&maximum| maximum < initial) {
            return Err(value_error(format!(
                "the minimum size {initial} is above the maximum {maximum}"
            )));
        }
        let ty = if descriptor_address(descriptor)? {
            TableType::new64(element.clone(), initial as u64, maximum.map(|n| n as u64))
        } else {
            TableType::new(
                element.clone(),
                to_u32(initial)?,
                maximum.map(to_u32).transpose()?,
            )
        };
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

    /// The table's type, with its current length as `minimum`.
    ///
    /// @return array{element: string, minimum: int, maximum?: int, address?: 'i64'}
    pub fn r#type(&self) -> PhpResult<ZBox<ZendHashTable>> {
        self.store
            .with(|ctx| table_type(&self.inner.ty(&ctx), self.inner.size(&ctx)))
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
