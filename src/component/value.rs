//! Conversion between PHP values and component values, following jco's
//! mapping of WIT to JavaScript where PHP has an equivalent.

use ext_php_rs::class::RegisteredClass;
use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ArrayKey, ZendHashTable, Zval};
use wasmtime::StoreContextMut;
use wasmtime::component::Val;
use wasmtime::component::types::Type;

use crate::component::error::thrown;
use std::rc::Rc;

use crate::component::host_resource;
use crate::component::resource::{self, Resource};
use crate::component::wit_type;
use crate::error::error;
use crate::store::{self, HostState};
use crate::value::{ConvertError, debug_type, downcast};

/// A value of a WIT `variant`: the name of its case and the case's payload.
#[php_class]
#[php(name = "Wasm\\Component\\Variant")]
#[php(flags = ClassFlags::Final)]
pub struct Variant {
    tag: String,
    value: Zval,
}

#[php_impl]
impl Variant {
    pub fn __construct(tag: String, value: Option<&Zval>) -> Self {
        Self {
            tag,
            value: value.map_or_else(Zval::null, Zval::shallow_clone),
        }
    }

    #[php(getter)]
    pub fn get_tag(&self) -> String {
        self.tag.clone()
    }

    #[php(getter)]
    pub fn get_value(&self) -> Zval {
        self.value.shallow_clone()
    }
}

/// A value of a WIT `result` inside another value: ok with a value, or err
/// with a payload.
#[php_class]
#[php(name = "Wasm\\Component\\Result")]
#[php(flags = ClassFlags::Final)]
pub struct ResultValue {
    ok: bool,
    value: Zval,
}

#[php_impl]
impl ResultValue {
    pub fn ok(value: Option<&Zval>) -> Self {
        Self::new(true, value)
    }

    pub fn err(error: Option<&Zval>) -> Self {
        Self::new(false, error)
    }

    /// Whether this is an ok result, as a property for var_dump() and assertEquals().
    #[php(getter)]
    pub fn get_ok(&self) -> bool {
        self.ok
    }

    /// The ok value or the err payload, as a property for var_dump() and assertEquals().
    #[php(getter)]
    pub fn get_payload(&self) -> Zval {
        self.value.shallow_clone()
    }

    pub fn is_ok(&self) -> bool {
        self.ok
    }

    pub fn is_err(&self) -> bool {
        !self.ok
    }

    /// The ok value; throws the err payload as a ComponentError.
    pub fn value(&self) -> PhpResult<Zval> {
        if self.ok {
            Ok(self.value.shallow_clone())
        } else {
            Err(thrown(self.value.shallow_clone()))
        }
    }

    /// The err payload; throws for an ok result.
    pub fn error(&self) -> PhpResult<Zval> {
        if self.ok {
            Err(error("the result is ok, it has no error"))
        } else {
            Ok(self.value.shallow_clone())
        }
    }
}

impl ResultValue {
    fn new(ok: bool, value: Option<&Zval>) -> Self {
        Self {
            ok,
            value: value.map_or_else(Zval::null, Zval::shallow_clone),
        }
    }
}

/// Converts a PHP value to a component value of type `ty`.
pub fn to_val(
    ctx: &mut StoreContextMut<'_, HostState>,
    value: &Zval,
    ty: &Type,
) -> Result<Val, ConvertError> {
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
        Type::Float32 => {
            let n = float(value, ty)?;
            let narrowed = n as f32;
            if n.is_finite() && narrowed.is_infinite() {
                return Err(ConvertError::Value(format!("{n} is out of range for f32")));
            }
            Val::Float32(narrowed)
        }
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
        Type::List(list) if matches!(list.ty(), Type::U8) => Val::List(
            value
                .zend_str()
                .filter(|_| value.is_string())
                .ok_or_else(|| mismatch(value, ty))?
                .as_bytes()
                .iter()
                .map(|byte| Val::U8(*byte))
                .collect(),
        ),
        Type::List(list) => {
            let element = list.ty();
            Val::List(
                list_array(value, ty)?
                    .values()
                    .map(|item| to_val(ctx, item, &element))
                    .collect::<Result<_, _>>()?,
            )
        }
        Type::Tuple(tuple) => {
            let items = list_array(value, ty)?;
            if items.len() != tuple.types().len() {
                return Err(ConvertError::Type(format!(
                    "expected a list of {} values for {}, got {}",
                    tuple.types().len(),
                    wit_type(ty),
                    items.len()
                )));
            }
            Val::Tuple(
                items
                    .values()
                    .zip(tuple.types())
                    .map(|(item, ty)| to_val(ctx, item, &ty))
                    .collect::<Result<_, _>>()?,
            )
        }
        Type::Record(record) => {
            let fields = assoc_array(
                value,
                ty,
                &record.fields().map(|f| camel(f.name)).collect::<Vec<_>>(),
            )?;
            let mut vals = Vec::with_capacity(record.fields().len());
            for field in record.fields() {
                let key = camel(field.name);
                let val = match fields.get(key.as_str()) {
                    Some(item) => to_val(ctx, item, &field.ty)?,
                    None if matches!(field.ty, Type::Option(_)) => Val::Option(None),
                    None => {
                        return Err(ConvertError::Type(format!(
                            "missing field \"{key}\" for {}",
                            wit_type(ty)
                        )));
                    }
                };
                vals.push((field.name.to_string(), val));
            }
            Val::Record(vals)
        }
        Type::Variant(variant) => {
            let given = downcast::<Variant>(value).ok_or_else(|| mismatch(value, ty))?;
            let case = variant
                .cases()
                .find(|case| case.name == given.tag)
                .ok_or_else(|| unknown_case(&given.tag, ty))?;
            let payload = match &case.ty {
                Some(payload) => Some(Box::new(to_val(ctx, &given.value, payload)?)),
                None => None,
            };
            Val::Variant(given.tag.clone(), payload)
        }
        Type::Enum(cases) => {
            let name = utf8(value, ty)?;
            if !cases.names().any(|case| case == name) {
                return Err(unknown_case(name, ty));
            }
            Val::Enum(name.to_string())
        }
        Type::Option(option) => {
            if value.is_null() {
                return Ok(Val::Option(None));
            }
            let inner = option.ty();
            if let Type::Option(nested) = &inner {
                // null already means the outer none, so the inner option needs tags.
                let given = downcast::<Variant>(value).ok_or_else(|| mismatch(value, ty))?;
                let nested = match given.tag.as_str() {
                    "none" => Val::Option(None),
                    "some" => Val::Option(Some(Box::new(to_val(ctx, &given.value, &nested.ty())?))),
                    other => return Err(unknown_case(other, ty)),
                };
                return Ok(Val::Option(Some(Box::new(nested))));
            }
            Val::Option(Some(Box::new(to_val(ctx, value, &inner)?)))
        }
        Type::Result(result) => {
            let given = downcast::<ResultValue>(value).ok_or_else(|| mismatch(value, ty))?;
            let payload_ty = if given.ok { result.ok() } else { result.err() };
            let payload = match payload_ty {
                Some(payload_ty) => Some(Box::new(to_val(ctx, &given.value, &payload_ty)?)),
                None => None,
            };
            Val::Result(if given.ok { Ok(payload) } else { Err(payload) })
        }
        Type::Flags(flags) => {
            let names: Vec<String> = flags.names().map(camel).collect();
            let given = assoc_array(value, ty, &names)?;
            let mut set = Vec::new();
            for (name, key) in flags.names().zip(&names) {
                match given.get(key.as_str()) {
                    None => {}
                    Some(flag) if flag.is_bool() => {
                        if flag.bool() == Some(true) {
                            set.push(name.to_string());
                        }
                    }
                    Some(flag) => {
                        return Err(ConvertError::Type(format!(
                            "expected bool for flag \"{key}\", got {}",
                            debug_type(flag)
                        )));
                    }
                }
            }
            Val::Flags(set)
        }
        Type::Own(resource_ty) | Type::Borrow(resource_ty) => {
            if let Some(handle) =
                host_resource::lower(ctx, value, resource_ty, matches!(ty, Type::Own(_)))?
            {
                return Ok(Val::Resource(handle));
            }
            let resource = downcast::<Resource>(value).ok_or_else(|| mismatch(value, ty))?;
            let handle = resource
                .handle()
                .map_err(|message| ConvertError::Error(message.to_string()))?;
            if !Rc::ptr_eq(resource.store(), &store::of(&*ctx)) || handle.ty() != *resource_ty {
                return Err(ConvertError::Type(format!(
                    "expected a resource of the type {} expects, got one of another type or instance",
                    wit_type(ty)
                )));
            }
            if matches!(ty, Type::Own(_)) {
                Val::Resource(resource::take_for_own(ctx, resource).map_err(ConvertError::Value)?)
            } else {
                Val::Resource(handle)
            }
        }
        other => return Err(unsupported(other)),
    })
}

/// Converts a component value of type `ty` to PHP.
pub fn from_val(
    ctx: &mut StoreContextMut<'_, HostState>,
    val: &Val,
    ty: &Type,
) -> Result<Zval, ConvertError> {
    let object = |object: std::result::Result<Zval, ext_php_rs::error::Error>| {
        object.map_err(|err| ConvertError::Value(err.to_string()))
    };
    match (val, ty) {
        (Val::List(items), Type::List(list)) if matches!(list.ty(), Type::U8) => {
            let bytes: Vec<u8> = items
                .iter()
                .map(|item| match item {
                    Val::U8(byte) => *byte,
                    _ => 0,
                })
                .collect();
            let mut zval = Zval::new();
            zval.set_binary(bytes);
            return Ok(zval);
        }
        (Val::List(items), Type::List(list)) => {
            let element = list.ty();
            return list_of(items.iter().map(|item| from_val(ctx, item, &element)));
        }
        (Val::Tuple(items), Type::Tuple(tuple)) => {
            return list_of(
                items
                    .iter()
                    .zip(tuple.types())
                    .map(|(item, ty)| from_val(ctx, item, &ty)),
            );
        }
        (Val::Record(fields), Type::Record(record)) => {
            let mut table = ZendHashTable::new();
            for ((name, item), field) in fields.iter().zip(record.fields()) {
                table
                    .insert(camel(name).as_str(), from_val(ctx, item, &field.ty)?)
                    .map_err(|err| ConvertError::Value(err.to_string()))?;
            }
            return object(table.into_zval(false));
        }
        (Val::Variant(tag, payload), Type::Variant(variant)) => {
            let payload_ty = variant
                .cases()
                .find(|case| case.name == tag)
                .and_then(|case| case.ty);
            let value = match (payload, payload_ty) {
                (Some(payload), Some(payload_ty)) => from_val(ctx, payload, &payload_ty)?,
                _ => Zval::null(),
            };
            return object(
                Variant {
                    tag: tag.clone(),
                    value,
                }
                .into_zval(false),
            );
        }
        (Val::Enum(name), Type::Enum(_)) => {
            let mut zval = Zval::new();
            set_string(&mut zval, name)?;
            return Ok(zval);
        }
        (Val::Option(None), Type::Option(_)) => return Ok(Zval::null()),
        (Val::Option(Some(inner)), Type::Option(option)) => {
            let inner_ty = option.ty();
            if let (Type::Option(nested), Val::Option(nested_val)) = (&inner_ty, &**inner) {
                let variant = match nested_val {
                    None => Variant {
                        tag: "none".into(),
                        value: Zval::null(),
                    },
                    Some(value) => Variant {
                        tag: "some".into(),
                        value: from_val(ctx, value, &nested.ty())?,
                    },
                };
                return object(variant.into_zval(false));
            }
            return from_val(ctx, inner, &inner_ty);
        }
        (Val::Result(outcome), Type::Result(result)) => {
            let (ok, payload, payload_ty) = match outcome {
                Ok(payload) => (true, payload, result.ok()),
                Err(payload) => (false, payload, result.err()),
            };
            let value = match (payload, payload_ty) {
                (Some(payload), Some(payload_ty)) => from_val(ctx, payload, &payload_ty)?,
                _ => Zval::null(),
            };
            return object(ResultValue { ok, value }.into_zval(false));
        }
        (Val::Resource(handle), Type::Own(_) | Type::Borrow(_)) => {
            if let Some(object) = host_resource::lift(ctx, handle)? {
                return Ok(object);
            }
            if !handle.owned() {
                return Err(ConvertError::Runtime(
                    "borrowed resources of another component are not supported yet".into(),
                ));
            }
            let store = store::of(&*ctx);
            let meta = store
                .resource_types
                .borrow()
                .iter()
                .find(|meta| meta.ty == handle.ty())
                .cloned();
            return object(Resource::new(store, meta, *handle).into_zval(false));
        }
        (Val::Stream(stream), Type::Stream(ty)) => {
            return crate::component::stream::Stream::lift(ctx, stream, ty.ty());
        }
        (Val::Flags(set), Type::Flags(flags)) => {
            let mut table = ZendHashTable::new();
            for name in flags.names() {
                table
                    .insert(camel(name).as_str(), set.iter().any(|flag| flag == name))
                    .map_err(|err| ConvertError::Value(err.to_string()))?;
            }
            return object(table.into_zval(false));
        }
        _ => {}
    }
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

/// Makes `==` and PHPUnit's `assertEquals()` compare Variant and Result by
/// value. PHP's default compares property tables, which these classes keep
/// in Rust fields instead, so any two of them looked equal.
pub fn install_comparison() {
    let install =
        |handlers: &ext_php_rs::zend::ZendObjectHandlers,
         compare: unsafe extern "C" fn(*mut Zval, *mut Zval) -> std::ffi::c_int| {
            let handlers = std::ptr::from_ref(handlers).cast_mut();
            // SAFETY: ext-php-rs hands out the handlers from a OnceCell that
            // lives for the whole process and has no other writer. This runs
            // once, at the first request startup, before any object of these
            // classes exists, so nothing reads the field while it changes.
            unsafe { (*handlers).compare = Some(compare) };
        };
    install(Variant::get_metadata().handlers(), compare::<Variant>);
    install(
        ResultValue::get_metadata().handlers(),
        compare::<ResultValue>,
    );
}

trait SameValue {
    fn key(&self) -> (&str, &Zval);
}

impl SameValue for Variant {
    fn key(&self) -> (&str, &Zval) {
        (&self.tag, &self.value)
    }
}

impl SameValue for ResultValue {
    fn key(&self) -> (&str, &Zval) {
        (if self.ok { "ok" } else { "err" }, &self.value)
    }
}

unsafe extern "C" fn compare<T: RegisteredClass + SameValue>(
    a: *mut Zval,
    b: *mut Zval,
) -> std::ffi::c_int {
    // SAFETY: the engine passes two live zvals, at least one of them an object.
    let (left, right) = unsafe { (&*a, &*b) };
    let (Some(left), Some(right)) = (downcast::<T>(left), downcast::<T>(right)) else {
        // Not two objects of this class: PHP's own rules apply.
        return unsafe { zend_std_compare_objects(a, b) };
    };
    let ((left_tag, left_value), (right_tag, right_value)) = (left.key(), right.key());
    if left_tag != right_tag {
        return 1;
    }
    let values = [left_value.shallow_clone(), right_value.shallow_clone()];
    let [mut x, mut y] = values;
    // SAFETY: two live zvals owned by this frame.
    unsafe { zend_compare(&raw mut x, &raw mut y) }
}

unsafe extern "C" {
    fn zend_std_compare_objects(a: *mut Zval, b: *mut Zval) -> std::ffi::c_int;
}

// zend_compare is ZEND_FASTCALL, which PHP for Windows defines as __vectorcall.
#[cfg(windows)]
unsafe extern "vectorcall" {
    fn zend_compare(a: *mut Zval, b: *mut Zval) -> std::ffi::c_int;
}

#[cfg(not(windows))]
unsafe extern "C" {
    fn zend_compare(a: *mut Zval, b: *mut Zval) -> std::ffi::c_int;
}

/// A top-level `result` return: the ok value, or its err thrown as a
/// ComponentError.
pub fn unwrap_result(
    ctx: &mut StoreContextMut<'_, HostState>,
    val: &Val,
    ty: &Type,
) -> PhpResult<Zval> {
    let (Val::Result(outcome), Type::Result(result)) = (val, ty) else {
        return Ok(from_val(ctx, val, ty)?);
    };
    let mut payload = |payload: &Option<Box<Val>>, payload_ty: Option<Type>| -> PhpResult<Zval> {
        Ok(match (payload, payload_ty) {
            (Some(payload), Some(payload_ty)) => from_val(ctx, payload, &payload_ty)?,
            _ => Zval::null(),
        })
    };
    match outcome {
        Ok(value) => payload(value, result.ok()),
        Err(error) => Err(thrown(payload(error, result.err())?)),
    }
}

fn list_of(items: impl Iterator<Item = Result<Zval, ConvertError>>) -> Result<Zval, ConvertError> {
    let mut table = ZendHashTable::new();
    for item in items {
        table
            .push(item?)
            .map_err(|err| ConvertError::Value(err.to_string()))?;
    }
    table
        .into_zval(false)
        .map_err(|err| ConvertError::Value(err.to_string()))
}

fn list_array<'a>(value: &'a Zval, ty: &Type) -> Result<&'a ZendHashTable, ConvertError> {
    value
        .array()
        .filter(|items| items.has_sequential_keys())
        .ok_or_else(|| {
            ConvertError::Type(format!(
                "expected list array for {}, got {}",
                wit_type(ty),
                debug_type(value)
            ))
        })
}

/// An array whose keys must all be among `known`.
fn assoc_array<'a>(
    value: &'a Zval,
    ty: &Type,
    known: &[String],
) -> Result<&'a ZendHashTable, ConvertError> {
    let table = value.array().ok_or_else(|| {
        ConvertError::Type(format!(
            "expected array for {}, got {}",
            wit_type(ty),
            debug_type(value)
        ))
    })?;
    for (key, _) in table.iter() {
        let key = match key {
            ArrayKey::Long(n) => n.to_string(),
            other => other.to_string(),
        };
        if !known.contains(&key) {
            return Err(ConvertError::Value(format!(
                "unknown key \"{key}\" for {}, expected one of {}",
                wit_type(ty),
                known.join(", ")
            )));
        }
    }
    Ok(table)
}

fn unknown_case(name: &str, ty: &Type) -> ConvertError {
    ConvertError::Value(format!("unknown case \"{name}\" for {}", wit_type(ty)))
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
        Type::Char | Type::String | Type::Enum(_) => "string",
        Type::List(list) if matches!(list.ty(), Type::U8) => "string",
        Type::List(_) | Type::Tuple(_) => "list array",
        Type::Record(_) | Type::Flags(_) => "array",
        Type::Variant(_) | Type::Option(_) => "Wasm\\Component\\Variant",
        Type::Result(_) => "Wasm\\Component\\Result",
        Type::Own(_) | Type::Borrow(_) => "Wasm\\Component\\Resource",
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
