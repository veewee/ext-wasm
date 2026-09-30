use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::{GlobalType, Mutability, Val, ValType};

use crate::error::{error, link_error, type_error};
use crate::store::{self, SharedStore};
use crate::value::{default_val, descriptor_bool, descriptor_str, from_val, parse_val_type, to_val};

/// A wasm global, like JS `WebAssembly.Global`. Named GlobalVar because
/// `Global` is a reserved word in PHP.
///
/// @property mixed $value
#[php_class]
#[php(name = "Wasm\\GlobalVar")]
#[php(flags = ClassFlags::Final)]
pub struct GlobalVar {
    pub store: SharedStore,
    pub inner: wasmtime::Global,
}

#[php_impl]
impl GlobalVar {
    /// @param array{value: string, mutable?: bool} $descriptor
    pub fn __construct(descriptor: &ZendHashTable, value: Option<&Zval>) -> PhpResult<Self> {
        let ty = descriptor_str(descriptor, "value")?
            .ok_or_else(|| type_error("descriptor \"value\" is required"))?;
        let ty = parse_val_type(ty)?;
        let mutability = if descriptor_bool(descriptor, "mutable")? { Mutability::Var } else { Mutability::Const };
        let initial = match value {
            Some(value) if !value.is_null() => to_val(value, &ty)?,
            _ => default_val(&ty),
        };
        let store = store::current();
        let inner = new_global(&store, ty, mutability, initial)?;
        Ok(Self { store, inner })
    }

    pub fn __get(&self, name: String) -> PhpResult<Zval> {
        property(&name)?;
        self.value_of()
    }

    pub fn __set(&self, name: String, value: &Zval) -> PhpResult<()> {
        property(&name)?;
        self.store.with(|mut ctx| {
            let ty = self.inner.ty(&ctx);
            if ty.mutability() == Mutability::Const {
                return Err(type_error("cannot set the value of an immutable global"));
            }
            let val = to_val(value, ty.content())?;
            self.inner.set(&mut ctx, val).map_err(|err| type_error(format!("{err:#}")))
        })
    }

    pub fn __isset(&self, name: String) -> bool {
        name == "value"
    }

    pub fn value_of(&self) -> PhpResult<Zval> {
        Ok(self.store.with(|mut ctx| from_val(&self.inner.get(&mut ctx)))?)
    }
}

// A real property needs an infallible getter, and reading a value can fail.
fn property(name: &str) -> PhpResult<()> {
    if name == "value" { Ok(()) } else { Err(error(format!("undefined property GlobalVar::${name}"))) }
}

pub fn new_global(store: &SharedStore, ty: ValType, mutability: Mutability, value: Val) -> PhpResult<wasmtime::Global> {
    store.with(|mut ctx| {
        wasmtime::Global::new(&mut ctx, GlobalType::new(ty, mutability), value).map_err(link_error)
    })
}
