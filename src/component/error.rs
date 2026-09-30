use ext_php_rs::args::Arg;
use ext_php_rs::builders::{ClassBuilder, ClassProperty};
use ext_php_rs::class::RegisteredClass;
use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpException;
use ext_php_rs::flags::{DataType, PropertyFlags};
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendObject, Zval};
use ext_php_rs::zend::{ClassEntry, ExecuteData, ce};

/// The err of a component function whose own return type is a `result`.
///
/// A PHP import throws it to return an err to the component.
///
/// @property mixed $payload
#[php_class]
#[php(name = "Wasm\\Exception\\ComponentError")]
#[php(extends(ce = crate::error::wasm_exception_ce, stub = "\\Wasm\\Exception\\WasmException"))]
#[php(modifier = declare_properties)]
#[derive(Default)]
pub struct ComponentError;

#[php_impl]
impl ComponentError {
    // Only declares the signature; `construct` below is installed as the handler.
    pub fn __construct(payload: Option<&Zval>) -> Self {
        let _ = payload;
        Self
    }
}

fn declare_properties(builder: ClassBuilder) -> ClassBuilder {
    builder.property(ClassProperty {
        name: "payload".into(),
        flags: PropertyFlags::Public,
        // Defaults of internal classes must not be refcounted.
        default: Some(Box::new(|| Ok(Zval::null()))),
        docs: &[],
        ty: None,
        nullable: true,
        readonly: false,
        default_stub: Some("null".into()),
    })
}

pub fn class_entry() -> &'static ClassEntry {
    ComponentError::get_metadata().ce()
}

ext_php_rs::zend_fastcall! {
    /// `new ComponentError(mixed $payload = null)`.
    ///
    /// Replaces the generated constructor, which expects an object laid out by
    /// ext-php-rs; this class uses `\Exception`'s own object handler instead.
    pub extern fn construct(ex: *mut ExecuteData, _return_value: *mut Zval) {
        // SAFETY: the engine passes the live frame of this constructor call.
        let Some(ex) = (unsafe { ex.as_mut() }) else { return };
        let mut payload = Arg::new("payload", DataType::Mixed);
        let (parser, this) = ex.parser_object();
        if parser.arg(&mut payload).not_required().parse().is_err() {
            return;
        }
        let Some(this) = this else { return };
        let empty = Zval::null();
        let payload = payload.zval().map_or(&empty, |payload| &**payload);
        if let Err(err) = initialize(this, payload.shallow_clone()) {
            crate::error::error(format!("cannot create ComponentError: {err}")).throw();
        }
    }
}

fn initialize(this: &mut ZendObject, payload: Zval) -> ext_php_rs::error::Result<()> {
    // Like jco: a string payload is the message, anything else is referred to.
    let message = payload.str().map_or_else(
        || "component returned an error, see $payload".to_string(),
        str::to_string,
    );
    // SAFETY: `this` is a live object of a class extending \Exception, whose
    // protected `message` property this writes in \Exception's own scope.
    unsafe {
        zend_update_property_stringl(
            ce::exception() as *const ClassEntry as *mut ClassEntry,
            this,
            c"message".as_ptr(),
            "message".len(),
            message.as_ptr().cast(),
            message.len(),
        );
    }
    this.set_property("payload", payload)?;
    Ok(())
}

unsafe extern "C" {
    fn zend_update_property_stringl(
        scope: *mut ClassEntry,
        object: *mut ZendObject,
        name: *const std::ffi::c_char,
        name_length: usize,
        value: *const std::ffi::c_char,
        value_length: usize,
    );
}

/// A thrown `ComponentError` carrying `payload`.
pub fn thrown(payload: Zval) -> PhpException {
    let mut object = ZendObject::new(class_entry());
    let created = initialize(&mut object, payload).and_then(|()| object.into_zval(false));
    match created {
        Ok(object) => PhpException::from_message(String::new()).with_object(object),
        Err(err) => crate::error::error(format!("cannot create ComponentError: {err}")),
    }
}
