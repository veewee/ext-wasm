use ext_php_rs::args::Arg;
use ext_php_rs::builders::{ClassBuilder, ClassProperty};
use ext_php_rs::class::RegisteredClass;
use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::{PhpException, PhpResult};
use ext_php_rs::flags::{DataType, PropertyFlags};
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, ZendObject, Zval};
use ext_php_rs::zend::{ClassEntry, ExecuteData, ExecutorGlobals};
use wasmtime::{ExnRef, ExnRefPre, ExnType, Rooted, StoreContextMut, Val};

use crate::error::{error, type_error, value_error};
use crate::store::HostState;
use crate::tag::{Tag, remember_tag, tag_to_zval};
use crate::value::{debug_type, downcast, from_val, to_val};

/// A wasm exception, like JS `WebAssembly.Exception`.
///
/// Thrown in PHP when a wasm exception escapes to PHP, and thrown by a PHP
/// callback to raise an exception that wasm code can catch.
///
/// @property \Wasm\Tag $tag
/// @property list<mixed> $payload
#[php_class]
#[php(name = "Wasm\\Exception\\WasmThrow")]
#[php(extends(ce = crate::error::wasm_exception_ce, stub = "\\Wasm\\Exception\\WasmException"))]
#[php(modifier = declare_properties)]
#[derive(Default)]
pub struct WasmThrow;

#[php_impl]
impl WasmThrow {
    // Only declares the signature; `construct` below is installed as the handler.
    pub fn __construct(tag: &Tag, payload: Option<&ZendHashTable>) -> Self {
        let _ = (tag, payload);
        Self
    }
}

fn declare_properties(builder: ClassBuilder) -> ClassBuilder {
    ["tag", "payload"]
        .into_iter()
        .fold(builder, |builder, name| {
            builder.property(ClassProperty {
                name: name.into(),
                flags: PropertyFlags::Public,
                // Defaults of internal classes must not be refcounted, so the payload starts as null too.
                default: Some(Box::new(|| Ok(Zval::null()))),
                docs: &[],
                ty: None,
                nullable: true,
                readonly: false,
                default_stub: Some("null".into()),
            })
        })
}

pub fn class_entry() -> &'static ClassEntry {
    WasmThrow::get_metadata().ce()
}

ext_php_rs::zend_fastcall! {
    /// `new WasmThrow(Tag $tag, array $payload = [])`.
    ///
    /// Replaces the generated constructor, which expects an object laid out by
    /// ext-php-rs; this class uses `\Exception`'s own object handler instead.
    pub extern fn construct(ex: *mut ExecuteData, _return_value: *mut Zval) {
        // SAFETY: the engine passes the live frame of this constructor call.
        let Some(ex) = (unsafe { ex.as_mut() }) else { return };
        // Types are checked in `initialize`, which reports them as PHP would.
        let mut tag = Arg::new("tag", DataType::Mixed);
        let mut payload = Arg::new("payload", DataType::Mixed);
        let (parser, this) = ex.parser_object();
        if parser.arg(&mut tag).not_required().arg(&mut payload).parse().is_err() {
            return;
        }
        let Some(this) = this else { return };
        let empty = Zval::null();
        let tag = tag.zval().map_or(&empty, |tag| &**tag);
        let payload = payload.zval().map_or(&empty, |payload| &**payload);
        if let Err(err) = initialize(this, tag, payload) {
            err.throw();
        }
    }
}

fn initialize(this: &mut ZendObject, tag_zval: &Zval, payload: &Zval) -> PhpResult<()> {
    let tag = downcast::<Tag>(tag_zval).ok_or_else(|| {
        type_error(format!(
            "WasmThrow::__construct(): Argument #1 ($tag) must be of type Wasm\\Tag, {} given",
            debug_type(tag_zval)
        ))
    })?;
    let values: Vec<&Zval> = match payload.array() {
        Some(list) => list.values().collect(),
        None if payload.is_null() => Vec::new(),
        None => {
            return Err(type_error(format!(
                "WasmThrow::__construct(): Argument #2 ($payload) must be of type array, {} given",
                debug_type(payload)
            )));
        }
    };
    tag.store.with(|mut ctx| {
        // Validating now reports a wrong payload where it was written, not where wasm catches it.
        fields(&mut ctx, &tag.inner, &values)?;
        remember_tag(&mut ctx, &tag.inner, tag_zval);
        Ok::<_, PhpException>(())
    })?;
    let mut list = ZendHashTable::new();
    for value in values {
        list.push(value.shallow_clone())?;
    }
    this.set_property("tag", tag_zval.shallow_clone())?;
    this.set_property("payload", list)?;
    Ok(())
}

fn fields(
    ctx: &mut StoreContextMut<'_, HostState>,
    tag: &wasmtime::Tag,
    values: &[&Zval],
) -> PhpResult<Vec<Val>> {
    let ty = tag.ty(&*ctx);
    let params: Vec<_> = ty.ty().params().collect();
    if params.len() != values.len() {
        return Err(value_error(format!(
            "tag expects a payload of {} value(s), {} given",
            params.len(),
            values.len()
        )));
    }
    values
        .iter()
        .zip(&params)
        .map(|(value, ty)| Ok(to_val(ctx, value, ty)?))
        .collect()
}

/// Turns a wasm exception that reached PHP into a thrown `WasmThrow`.
pub fn from_wasm(
    ctx: &mut StoreContextMut<'_, HostState>,
    exception: Rooted<ExnRef>,
) -> PhpException {
    match to_php(ctx, exception) {
        Ok(object) => PhpException::from_message(String::new()).with_object(object),
        Err(err) => err,
    }
}

fn to_php(ctx: &mut StoreContextMut<'_, HostState>, exception: Rooted<ExnRef>) -> PhpResult<Zval> {
    let failed = |err: wasmtime::Error| error(format!("cannot read wasm exception: {err:#}"));
    let tag = exception.tag(&mut *ctx).map_err(failed)?;
    let count = tag.ty(&*ctx).ty().params().len();
    let mut payload = ZendHashTable::new();
    for index in 0..count {
        let field = exception.field(&mut *ctx, index).map_err(failed)?;
        payload.push(from_val(ctx, &field)?)?;
    }
    // Going through the class's create_object gives the file, line and trace of the PHP caller.
    let mut object = ZendObject::new(class_entry());
    object.set_property("tag", tag_to_zval(ctx, &tag)?)?;
    object.set_property("payload", payload)?;
    Ok(object.into_zval(false)?)
}

/// Takes a pending PHP `WasmThrow`, if any, and turns it into a wasm exception.
pub fn take_pending(
    ctx: &mut StoreContextMut<'_, HostState>,
) -> Option<wasmtime::Result<Rooted<ExnRef>>> {
    let pending = ExecutorGlobals::get()
        .exception()
        .is_some_and(|object| object.instance_of(class_entry()));
    if !pending {
        return None;
    }
    let object = ExecutorGlobals::take_exception()?;
    Some(to_wasm(ctx, &object))
}

fn to_wasm(
    ctx: &mut StoreContextMut<'_, HostState>,
    object: &ZendObject,
) -> wasmtime::Result<Rooted<ExnRef>> {
    let invalid = |message: &str| wasmtime::Error::msg(format!("invalid WasmThrow: {message}"));
    let tag_zval: &Zval = object
        .get_property("tag")
        .map_err(|_| invalid("missing tag"))?;
    let tag = downcast::<Tag>(tag_zval)
        .ok_or_else(|| invalid("tag is not a Wasm\\Tag"))?
        .inner;
    let payload: &Zval = object
        .get_property("payload")
        .map_err(|_| invalid("missing payload"))?;
    let values: Vec<&Zval> = payload
        .array()
        .map(|list| list.values().collect())
        .unwrap_or_default();
    let fields = {
        let ty = tag.ty(&*ctx);
        let params: Vec<_> = ty.ty().params().collect();
        if params.len() != values.len() {
            return Err(invalid("payload does not match the tag"));
        }
        values
            .iter()
            .zip(&params)
            .map(|(value, ty)| to_val(ctx, value, ty))
            .collect::<Result<Vec<Val>, _>>()?
    };
    let ty = ExnType::from_tag_type(&tag.ty(&*ctx))?;
    let allocator = ExnRefPre::new(&mut *ctx, ty);
    ExnRef::new(&mut *ctx, &allocator, &tag, &fields)
}

/// The thrown exception of a failed call: a `WasmThrow` for wasm exceptions,
/// a `RuntimeError` for traps.
pub fn call_error(ctx: &mut StoreContextMut<'_, HostState>, err: wasmtime::Error) -> PhpException {
    if err.is::<wasmtime::ThrownException>()
        && let Some(exception) = ctx.take_pending_exception()
    {
        return from_wasm(ctx, exception);
    }
    crate::error::runtime_error(err)
}
