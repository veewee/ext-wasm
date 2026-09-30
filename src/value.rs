use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::DataType;
use ext_php_rs::types::{ZendHashTable, Zval};
use ext_php_rs::convert::IntoZval;
use wasmtime::{Val, ValType};

use crate::error::{type_error, value_error};

/// Converts a PHP value to a wasm value of type `ty`.
///
/// Stricter than JS on purpose: JS coerces anything with ToNumber, which turns
/// typos like `'1'` or `1.5` for an i32 into silent bugs.
pub fn to_val(value: &Zval, ty: &ValType) -> PhpResult<Val> {
    Ok(match ty {
        ValType::I32 => {
            let n = expect_int(value, ty)?;
            if n < i64::from(i32::MIN) || n > i64::from(u32::MAX) {
                return Err(value_error(format!("{n} is out of range for i32")));
            }
            // Truncation keeps the bit pattern, so unsigned input maps onto the signed value.
            Val::I32(n as i32)
        }
        ValType::I64 => Val::I64(expect_int(value, ty)?),
        ValType::F32 => Val::F32((expect_float(value, ty)? as f32).to_bits()),
        ValType::F64 => Val::F64(expect_float(value, ty)?.to_bits()),
        other => return Err(type_error(format!("unsupported wasm type {other}"))),
    })
}

pub fn from_val(val: &Val) -> PhpResult<Zval> {
    let mut zval = Zval::new();
    match val {
        Val::I32(n) => zval.set_long(*n),
        Val::I64(n) => zval.set_long(*n),
        Val::F32(bits) => zval.set_double(f32::from_bits(*bits)),
        Val::F64(bits) => zval.set_double(f64::from_bits(*bits)),
        other => return Err(type_error(format!("unsupported wasm value {other:?}"))),
    }
    Ok(zval)
}

/// JS semantics: no result is `null`, one result is the value, several are a list.
pub fn results_to_zval(results: &[Val]) -> PhpResult<Zval> {
    match results {
        [] => Ok(Zval::null()),
        [single] => from_val(single),
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

fn expect_int(value: &Zval, ty: &ValType) -> PhpResult<i64> {
    value
        .long()
        .ok_or_else(|| type_error(format!("expected int for {ty}, got {}", debug_type(value))))
}

fn expect_float(value: &Zval, ty: &ValType) -> PhpResult<f64> {
    value
        .double()
        .or_else(|| value.long().map(|n| n as f64))
        .ok_or_else(|| type_error(format!("expected int|float for {ty}, got {}", debug_type(value))))
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
