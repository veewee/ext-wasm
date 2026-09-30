use ext_php_rs::exception::{PhpException, PhpResult};
use ext_php_rs::flags::DataType;
use ext_php_rs::types::{ZendHashTable, Zval};
use ext_php_rs::class::RegisteredClass;
use ext_php_rs::convert::{FromZval, IntoZval};
use ext_php_rs::types::ZendClassObject;
use wasmtime::{Val, ValType};

use crate::error::{type_error, value_error};

/// A conversion failure, kept separate from `PhpException` so host functions
/// can turn it into a trap message instead of a thrown PHP exception.
#[derive(Debug)]
pub enum ConvertError {
    Type(String),
    Value(String),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Type(message) | Self::Value(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<ConvertError> for PhpException {
    fn from(err: ConvertError) -> Self {
        match err {
            ConvertError::Type(message) => type_error(message),
            ConvertError::Value(message) => value_error(message),
        }
    }
}

/// Converts a PHP value to a wasm value of type `ty`.
///
/// Stricter than JS on purpose: JS coerces anything with ToNumber, which turns
/// typos like `'1'` or `1.5` for an i32 into silent bugs.
pub fn to_val(value: &Zval, ty: &ValType) -> Result<Val, ConvertError> {
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
        other => return Err(ConvertError::Type(format!("unsupported wasm type {other}"))),
    })
}

pub fn from_val(val: &Val) -> Result<Zval, ConvertError> {
    let mut zval = Zval::new();
    match val {
        Val::I32(n) => zval.set_long(*n),
        Val::I64(n) => zval.set_long(*n),
        Val::F32(bits) => zval.set_double(f32::from_bits(*bits)),
        Val::F64(bits) => zval.set_double(f64::from_bits(*bits)),
        other => return Err(ConvertError::Type(format!("unsupported wasm value {other:?}"))),
    }
    Ok(zval)
}

/// JS semantics: no result is `null`, one result is the value, several are a list.
pub fn results_to_zval(results: &[Val]) -> PhpResult<Zval> {
    match results {
        [] => Ok(Zval::null()),
        [single] => Ok(from_val(single)?),
        many => {
            let mut list = ZendHashTable::new();
            for val in many {
                list.push(from_val(val)?)?;
            }
            Ok(list.into_zval(false)?)
        }
    }
}

pub fn default_val(ty: &ValType) -> Val {
    Val::default_for_ty(ty).unwrap_or(Val::I32(0))
}

fn expect_int(value: &Zval, ty: &ValType) -> Result<i64, ConvertError> {
    value
        .long()
        .ok_or_else(|| ConvertError::Type(format!("expected int for {ty}, got {}", debug_type(value))))
}

fn expect_float(value: &Zval, ty: &ValType) -> Result<f64, ConvertError> {
    value
        .double()
        .or_else(|| value.long().map(|n| n as f64))
        .ok_or_else(|| ConvertError::Type(format!("expected int|float for {ty}, got {}", debug_type(value))))
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

/// Parses a JS-style value type name ("i32", "f64", ...).
pub fn parse_val_type(name: &str) -> PhpResult<ValType> {
    Ok(match name {
        "i32" => ValType::I32,
        "i64" => ValType::I64,
        "f32" => ValType::F32,
        "f64" => ValType::F64,
        other => return Err(type_error(format!("unknown wasm value type \"{other}\""))),
    })
}

/// Returns the wrapped Rust struct when `value` is an instance of the PHP class `T`.
pub fn downcast<T: RegisteredClass>(value: &Zval) -> Option<&T> {
    <&ZendClassObject<T>>::from_zval(value).map(|object| &**object)
}

pub fn descriptor_str<'a>(descriptor: &'a ZendHashTable, key: &str) -> PhpResult<Option<&'a str>> {
    match descriptor.get(key) {
        None => Ok(None),
        Some(value) => value
            .str()
            .map(Some)
            .ok_or_else(|| type_error(format!("descriptor \"{key}\" must be a string, got {}", debug_type(value)))),
    }
}

pub fn descriptor_int(descriptor: &ZendHashTable, key: &str) -> PhpResult<Option<i64>> {
    match descriptor.get(key) {
        None => Ok(None),
        Some(value) if value.is_null() => Ok(None),
        Some(value) => match value.long() {
            Some(n) if n >= 0 => Ok(Some(n)),
            Some(n) => Err(value_error(format!("descriptor \"{key}\" must not be negative, got {n}"))),
            None => Err(type_error(format!("descriptor \"{key}\" must be an int, got {}", debug_type(value)))),
        },
    }
}

pub fn descriptor_bool(descriptor: &ZendHashTable, key: &str) -> PhpResult<bool> {
    match descriptor.get(key) {
        None => Ok(false),
        Some(value) => value
            .bool()
            .ok_or_else(|| type_error(format!("descriptor \"{key}\" must be a bool, got {}", debug_type(value)))),
    }
}
