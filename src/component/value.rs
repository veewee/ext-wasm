//! Conversion between PHP values and component values, following jco's
//! mapping of WIT to JavaScript where PHP has an equivalent.

use ext_php_rs::types::Zval;
use wasmtime::component::Val;
use wasmtime::component::types::Type;

use crate::component::wit_type;
use crate::value::{ConvertError, debug_type};

/// Converts a PHP value to a component value of type `ty`.
pub fn to_val(value: &Zval, ty: &Type) -> Result<Val, ConvertError> {
    Ok(match ty {
        Type::Bool => Val::Bool(
            value
                .bool()
                .filter(|_| value.is_bool())
                .ok_or_else(|| mismatch(value, ty))?,
        ),
        Type::S8 => Val::S8(ranged(value, ty)?),
        Type::U8 => Val::U8(ranged(value, ty)?),
        Type::S16 => Val::S16(ranged(value, ty)?),
        Type::U16 => Val::U16(ranged(value, ty)?),
        Type::S32 => Val::S32(ranged(value, ty)?),
        Type::U32 => Val::U32(ranged(value, ty)?),
        Type::S64 => Val::S64(int(value, ty)?),
        // PHP has no unsigned 64-bit integer, so the bit pattern carries over, as
        // with unpack('J') and core i64.
        Type::U64 => Val::U64(int(value, ty)? as u64),
        Type::Float32 => Val::Float32(float(value, ty)? as f32),
        Type::Float64 => Val::Float64(float(value, ty)?),
        Type::Char => {
            let text = utf8(value, ty)?;
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Val::Char(c),
                _ => {
                    return Err(ConvertError::Value(format!(
                        "expected exactly one character for char, got {} bytes",
                        text.len()
                    )));
                }
            }
        }
        Type::String => Val::String(utf8(value, ty)?.to_string()),
        other => return Err(unsupported(other)),
    })
}

/// Converts a component value of type `ty` to PHP.
pub fn from_val(val: &Val, ty: &Type) -> Result<Zval, ConvertError> {
    let _ = ty;
    let mut zval = Zval::new();
    match val {
        Val::Bool(b) => zval.set_bool(*b),
        Val::S8(n) => zval.set_long(i64::from(*n)),
        Val::U8(n) => zval.set_long(i64::from(*n)),
        Val::S16(n) => zval.set_long(i64::from(*n)),
        Val::U16(n) => zval.set_long(i64::from(*n)),
        Val::S32(n) => zval.set_long(i64::from(*n)),
        Val::U32(n) => zval.set_long(i64::from(*n)),
        Val::S64(n) => zval.set_long(*n),
        Val::U64(n) => zval.set_long(*n as i64),
        Val::Float32(n) => zval.set_double(f64::from(*n)),
        Val::Float64(n) => zval.set_double(*n),
        Val::Char(c) => set_string(&mut zval, c.encode_utf8(&mut [0; 4]))?,
        Val::String(s) => set_string(&mut zval, s)?,
        other => {
            return Err(ConvertError::Runtime(format!(
                "component value {other:?} is not supported yet"
            )));
        }
    }
    Ok(zval)
}

/// A WIT name as a PHP identifier: `add-numbers` becomes `addNumbers`, like jco.
pub fn camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (index, word) in name.split('-').enumerate() {
        let lower = word.to_ascii_lowercase();
        if index == 0 {
            out.push_str(&lower);
        } else {
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                out.push(first.to_ascii_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    out
}

fn set_string(zval: &mut Zval, text: &str) -> Result<(), ConvertError> {
    zval.set_string(text, false)
        .map_err(|err| ConvertError::Value(err.to_string()))
}

fn mismatch(value: &Zval, ty: &Type) -> ConvertError {
    ConvertError::Type(format!(
        "expected {} for {}, got {}",
        php_type(ty),
        wit_type(ty),
        debug_type(value)
    ))
}

fn php_type(ty: &Type) -> &'static str {
    match ty {
        Type::Bool => "bool",
        Type::Float32 | Type::Float64 => "int|float",
        Type::Char | Type::String => "string",
        _ => "int",
    }
}

fn unsupported(ty: &Type) -> ConvertError {
    ConvertError::Runtime(format!("{} is not supported yet", wit_type(ty)))
}

fn int(value: &Zval, ty: &Type) -> Result<i64, ConvertError> {
    value
        .long()
        .filter(|_| value.is_long())
        .ok_or_else(|| mismatch(value, ty))
}

fn ranged<T: TryFrom<i64>>(value: &Zval, ty: &Type) -> Result<T, ConvertError> {
    let n = int(value, ty)?;
    T::try_from(n)
        .map_err(|_| ConvertError::Value(format!("{n} is out of range for {}", wit_type(ty))))
}

fn float(value: &Zval, ty: &Type) -> Result<f64, ConvertError> {
    if value.is_double() {
        return Ok(value.double().unwrap_or_default());
    }
    Ok(int(value, ty).map_err(|_| mismatch(value, ty))? as f64)
}

fn utf8<'a>(value: &'a Zval, ty: &Type) -> Result<&'a str, ConvertError> {
    let bytes = value
        .zend_str()
        .filter(|_| value.is_string())
        .ok_or_else(|| mismatch(value, ty))?
        .as_bytes();
    std::str::from_utf8(bytes)
        .map_err(|_| ConvertError::Value(format!("{} must be valid UTF-8", wit_type(ty))))
}
