use ext_php_rs::class::RegisteredClass;
use ext_php_rs::convert::{FromZval, IntoZval};
use ext_php_rs::exception::{PhpException, PhpResult};
use ext_php_rs::flags::DataType;
use ext_php_rs::types::ZendClassObject;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::{ExternRef, HeapTopType, HeapType, Ref, RefType, StoreContextMut, Val, ValType};

use crate::error::{link_error, runtime_error, type_error, value_error};
use crate::func::Func;
use crate::store::{self, HostState, ValueKey};

/// A conversion failure, kept separate from `PhpException` so host functions
/// can turn it into a trap message instead of a thrown PHP exception.
#[derive(Debug)]
pub enum ConvertError {
    Type(String),
    Value(String),
    /// A wasm object from another store.
    Link(String),
    Runtime(String),
    /// A plain `\Error`, such as using a dropped resource.
    Error(String),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Type(message)
            | Self::Value(message)
            | Self::Link(message)
            | Self::Runtime(message)
            | Self::Error(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<ConvertError> for PhpException {
    fn from(err: ConvertError) -> Self {
        match err {
            ConvertError::Type(message) => type_error(message),
            ConvertError::Value(message) => value_error(message),
            ConvertError::Link(message) => link_error(message),
            ConvertError::Runtime(message) => runtime_error(wasmtime::Error::msg(message)),
            ConvertError::Error(message) => crate::error::error(message),
        }
    }
}

/// Converts a PHP value to a wasm value of type `ty`.
///
/// Stricter than JS on purpose: JS coerces anything with ToNumber, which turns
/// typos like `'1'` or `1.5` for an i32 into silent bugs.
pub fn to_val(
    ctx: &mut StoreContextMut<'_, HostState>,
    value: &Zval,
    ty: &ValType,
) -> Result<Val, ConvertError> {
    Ok(match ty {
        ValType::I32 => {
            let n = expect_int(value, ty)?;
            if n < i64::from(i32::MIN) || n > i64::from(u32::MAX) {
                return Err(ConvertError::Value(format!("{n} is out of range for i32")));
            }
            // Truncation keeps the bit pattern, so unsigned input maps onto the signed value.
            Val::I32(n as i32)
        }
        ValType::I64 => Val::I64(expect_int(value, ty)?),
        ValType::F32 => Val::F32((expect_float(value, ty)? as f32).to_bits()),
        ValType::F64 => Val::F64(expect_float(value, ty)?.to_bits()),
        ValType::V128 => {
            let bytes: [u8; 16] = value
                .zend_str()
                .and_then(|bytes| bytes.as_bytes().try_into().ok())
                .ok_or_else(|| {
                    ConvertError::Type(format!(
                        "expected a 16 byte string for v128, got {}",
                        debug_type(value)
                    ))
                })?;
            Val::V128(u128::from_le_bytes(bytes).into())
        }
        ValType::Ref(ref_type) => to_ref(ctx, value, ref_type)?.into(),
    })
}

pub fn to_ref(
    ctx: &mut StoreContextMut<'_, HostState>,
    value: &Zval,
    ty: &RefType,
) -> Result<Ref, ConvertError> {
    if value.is_null() {
        if !ty.is_nullable() {
            return Err(ConvertError::Type(format!(
                "{} needs a value, it cannot be null",
                crate::types::ref_type_name(ty)
            )));
        }
        return Ok(match ty.heap_type().top() {
            HeapTopType::Func => Ref::Func(None),
            HeapTopType::Extern => Ref::Extern(None),
            _ => {
                return Err(ConvertError::Type(format!(
                    "unsupported wasm type {}",
                    crate::types::ref_type_name(ty)
                )));
            }
        });
    }
    match ty.heap_type() {
        HeapType::NoExtern | HeapType::NoFunc => Err(ConvertError::Type(format!(
            "expected null for {}, got {}",
            crate::types::ref_type_name(ty),
            debug_type(value)
        ))),
        HeapType::Extern => {
            // Allocating can run the GC, which cannot see a parked call's frames.
            if store::of(&*ctx).is_parked() {
                return Err(ConvertError::Runtime(store::BUSY.into()));
            }
            let key = ctx.data_mut().values.insert_ref(value.shallow_clone());
            let externref = ExternRef::new(&mut *ctx, key)
                .map_err(|err| ConvertError::Value(format!("{err:#}")))?;
            Ok(Ref::Extern(Some(externref)))
        }
        HeapType::Func => match downcast::<Func>(value) {
            Some(func) if store::owns(&*ctx, &func.store) => Ok(Ref::Func(Some(func.inner))),
            Some(_) => Err(ConvertError::Link(store::mismatch_message("Func"))),
            None => Err(ConvertError::Type(format!(
                "expected Wasm\\Func or null for {ty}, got {}",
                debug_type(value)
            ))),
        },
        HeapType::ConcreteFunc(expected) => match downcast::<Func>(value) {
            // The store check comes first: a func of another store makes
            // wasmtime's type lookups panic.
            Some(func) if store::owns(&*ctx, &func.store) => {
                let actual = func.inner.ty(&*ctx);
                // Matched by declared type, as the spec and JS do: wasmtime's
                // own Func::matches_ty only compares parameters and results.
                if HeapType::ConcreteFunc(actual.clone()).matches(ty.heap_type()) {
                    Ok(Ref::Func(Some(func.inner)))
                } else {
                    let (wanted, given) = (
                        crate::types::signature(expected),
                        crate::types::signature(&actual),
                    );
                    // Two types can read the same and still not match by declaration.
                    let why = if wanted == given {
                        ", declared as another type that is not a subtype"
                    } else {
                        ""
                    };
                    Err(ConvertError::Type(format!(
                        "expected a Wasm\\Func of type {wanted}, got one of type {given}{why}"
                    )))
                }
            }
            Some(_) => Err(ConvertError::Link(store::mismatch_message("Func"))),
            None => Err(ConvertError::Type(format!(
                "expected a Wasm\\Func of type {}{}, got {}",
                crate::types::signature(expected),
                if ty.is_nullable() { " or null" } else { "" },
                debug_type(value)
            ))),
        },
        _ => Err(ConvertError::Type(format!(
            "unsupported wasm type {}",
            crate::types::ref_type_name(ty)
        ))),
    }
}

pub fn from_val(ctx: &mut StoreContextMut<'_, HostState>, val: &Val) -> Result<Zval, ConvertError> {
    let mut zval = Zval::new();
    match val {
        Val::I32(n) => zval.set_long(*n),
        Val::I64(n) => zval.set_long(*n),
        Val::F32(bits) => zval.set_double(f32::from_bits(*bits)),
        Val::F64(bits) => zval.set_double(f64::from_bits(*bits)),
        Val::V128(v) => zval.set_binary(v.as_u128().to_le_bytes().to_vec()),
        Val::FuncRef(func) => return from_ref(ctx, &Ref::Func(*func)),
        Val::ExternRef(externref) => return from_ref(ctx, &Ref::Extern(*externref)),
        other => {
            return Err(ConvertError::Type(format!(
                "unsupported wasm value {other:?}"
            )));
        }
    }
    Ok(zval)
}

pub fn from_ref(
    ctx: &mut StoreContextMut<'_, HostState>,
    value: &Ref,
) -> Result<Zval, ConvertError> {
    match value {
        Ref::Func(None) | Ref::Extern(None) => Ok(Zval::null()),
        Ref::Func(Some(inner)) => Func {
            store: store::of(ctx),
            inner: *inner,
        }
        .into_zval(false)
        .map_err(|err| ConvertError::Value(err.to_string())),
        Ref::Extern(Some(externref)) => {
            let key = externref
                .data(&*ctx)
                .ok()
                .flatten()
                .and_then(|data| data.downcast_ref::<ValueKey>())
                .map(ValueKey::key)
                .ok_or_else(|| ConvertError::Type("externref does not hold a PHP value".into()))?;
            Ok(ctx.data().values.get(key).shallow_clone())
        }
        other => Err(ConvertError::Type(format!(
            "unsupported wasm reference {other:?}"
        ))),
    }
}

/// JS semantics: no result is `null`, one result is the value, several are a list.
pub fn results_to_zval(
    ctx: &mut StoreContextMut<'_, HostState>,
    results: &[Val],
) -> PhpResult<Zval> {
    match results {
        [] => Ok(Zval::null()),
        [single] => Ok(from_val(ctx, single)?),
        many => {
            let mut list = ZendHashTable::new();
            for val in many {
                list.push(from_val(ctx, val)?)?;
            }
            Ok(list.into_zval(false)?)
        }
    }
}

pub fn default_val(ty: &ValType) -> Val {
    Val::default_for_ty(ty).unwrap_or(Val::I32(0))
}

fn expect_int(value: &Zval, ty: &ValType) -> Result<i64, ConvertError> {
    value.long().ok_or_else(|| {
        ConvertError::Type(format!("expected int for {ty}, got {}", debug_type(value)))
    })
}

fn expect_float(value: &Zval, ty: &ValType) -> Result<f64, ConvertError> {
    value
        .double()
        .or_else(|| value.long().map(|n| n as f64))
        .ok_or_else(|| {
            ConvertError::Type(format!(
                "expected int|float for {ty}, got {}",
                debug_type(value)
            ))
        })
}

/// Mirrors PHP's `get_debug_type()` for error messages.
pub fn debug_type(value: &Zval) -> String {
    match value.get_type() {
        DataType::Null | DataType::Undef => "null".into(),
        DataType::False | DataType::True | DataType::Bool => "bool".into(),
        DataType::Long => "int".into(),
        DataType::Double => "float".into(),
        DataType::String => "string".into(),
        DataType::Array => "array".into(),
        DataType::Object(_) => value
            .object()
            .and_then(|object| object.get_class_name().ok())
            .unwrap_or_else(|| "object".into()),
        DataType::Reference => debug_type(value.dereference()),
        other => other.to_string(),
    }
}

/// Parses a JS-style value type name ("i32", "f64", ...), or one of the
/// reference type names `type()` gives that PHP values can be converted to.
pub fn parse_val_type(name: &str) -> PhpResult<ValType> {
    let reference = |nullable, heap| ValType::Ref(RefType::new(nullable, heap));
    Ok(match name {
        "i32" => ValType::I32,
        "i64" => ValType::I64,
        "f32" => ValType::F32,
        "f64" => ValType::F64,
        "v128" => ValType::V128,
        "externref" | "(ref null extern)" => ValType::EXTERNREF,
        "anyfunc" | "funcref" | "(ref null func)" => ValType::FUNCREF,
        "nullexternref" => ValType::NULLEXTERNREF,
        "nullfuncref" => ValType::NULLFUNCREF,
        "(ref extern)" => reference(false, HeapType::Extern),
        "(ref func)" => reference(false, HeapType::Func),
        other => {
            return Err(type_error(format!(
                "wasm value type \"{other}\" is unknown or cannot hold a PHP value"
            )));
        }
    })
}

/// Whether a memory or table descriptor asks for a 64-bit address type.
pub fn descriptor_address(descriptor: &ZendHashTable) -> PhpResult<bool> {
    match descriptor_str(descriptor, "address")? {
        None | Some("i32") => Ok(false),
        Some("i64") => Ok(true),
        Some(other) => Err(type_error(format!(
            "descriptor \"address\" must be \"i32\" or \"i64\", got \"{other}\""
        ))),
    }
}

/// The value a global or table starts with when PHP gives none or null.
pub fn null_default(ty: &ValType) -> PhpResult<Val> {
    Val::default_for_ty(ty).ok_or_else(|| {
        type_error(format!(
            "{} needs a value, it cannot be null",
            crate::types::val_type_name(ty)
        ))
    })
}

/// Returns the wrapped Rust struct when `value` is an instance of the PHP class `T`.
pub fn downcast<T: RegisteredClass>(value: &Zval) -> Option<&T> {
    <&ZendClassObject<T>>::from_zval(value).map(|object| &**object)
}

pub fn descriptor_str<'a>(descriptor: &'a ZendHashTable, key: &str) -> PhpResult<Option<&'a str>> {
    match descriptor.get(key) {
        None => Ok(None),
        Some(value) => value.str().map(Some).ok_or_else(|| {
            type_error(format!(
                "descriptor \"{key}\" must be a string, got {}",
                debug_type(value)
            ))
        }),
    }
}

pub fn descriptor_int(descriptor: &ZendHashTable, key: &str) -> PhpResult<Option<i64>> {
    match descriptor.get(key) {
        None => Ok(None),
        Some(value) if value.is_null() => Ok(None),
        Some(value) => match value.long() {
            Some(n) if n >= 0 => Ok(Some(n)),
            Some(n) => Err(value_error(format!(
                "descriptor \"{key}\" must not be negative, got {n}"
            ))),
            None => Err(type_error(format!(
                "descriptor \"{key}\" must be an int, got {}",
                debug_type(value)
            ))),
        },
    }
}

/// The minimum size of a memory or table descriptor, given as `initial` like
/// the JS constructors or as `minimum` like the types `type()` returns.
pub fn descriptor_minimum(descriptor: &ZendHashTable) -> PhpResult<i64> {
    match (
        descriptor_int(descriptor, "initial")?,
        descriptor_int(descriptor, "minimum")?,
    ) {
        (Some(_), Some(_)) => Err(type_error(
            "descriptor takes \"initial\" and \"minimum\" as alternatives, not both",
        )),
        (Some(n), None) | (None, Some(n)) => Ok(n),
        (None, None) => Err(type_error("descriptor \"initial\" is required")),
    }
}

pub fn descriptor_bool(descriptor: &ZendHashTable, key: &str) -> PhpResult<bool> {
    match descriptor.get(key) {
        None => Ok(false),
        Some(value) => value.bool().ok_or_else(|| {
            type_error(format!(
                "descriptor \"{key}\" must be a bool, got {}",
                debug_type(value)
            ))
        }),
    }
}
