use std::fmt::Display;
use std::sync::Once;

use ext_php_rs::exception::PhpException;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::class::RegisteredClass;
use ext_php_rs::types::Zval;
use ext_php_rs::zend::{ce, ClassEntry};

#[php_class]
#[php(name = "Wasm\\Exception\\WasmException")]
#[php(extends(ce = ce::exception, stub = "\\Exception"))]
#[derive(Default)]
pub struct WasmException;

#[php_class]
#[php(name = "Wasm\\Exception\\CompileError")]
#[php(extends(ce = wasm_exception_ce, stub = "\\Wasm\\Exception\\WasmException"))]
#[derive(Default)]
pub struct CompileError;

#[php_class]
#[php(name = "Wasm\\Exception\\LinkError")]
#[php(extends(ce = wasm_exception_ce, stub = "\\Wasm\\Exception\\WasmException"))]
#[derive(Default)]
pub struct LinkError;

#[php_class]
#[php(name = "Wasm\\Exception\\RuntimeError")]
#[php(extends(ce = wasm_exception_ce, stub = "\\Wasm\\Exception\\WasmException"))]
#[derive(Default)]
pub struct RuntimeError;

// The explicit `ce` form is used so the generated stub gets a fully qualified parent name.
fn wasm_exception_ce() -> &'static ClassEntry {
    WasmException::get_metadata().ce()
}

// The constructor is only declared so the generated stubs show the signature
// PHP users get; `adopt_exception_behaviour` replaces it with \Exception's own.
macro_rules! exception_constructor {
    ($($class:ident),*) => {$(
        #[php_impl]
        impl $class {
            pub fn __construct(message: Option<String>, code: Option<i64>, previous: Option<&Zval>) -> Self {
                let _ = (message, code, previous);
                Self
            }
        }
    )*};
}

exception_constructor!(WasmException, CompileError, LinkError, RuntimeError);

/// Makes the error classes behave like any `\Exception` subclass written in PHP.
///
/// ext-php-rs gives every class its own `create_object` handler and constructor.
/// For exceptions that loses the file, line and trace that `\Exception`'s
/// handler records, the inherited constructor never runs and serialization is
/// denied. The error classes carry no Rust state, so they can use `\Exception`'s
/// handlers directly.
///
/// Classes are registered after the module startup hook, so this runs at the
/// first request startup instead.
pub fn adopt_exception_behaviour() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let base: *const ClassEntry = ce::exception();
        let classes: [*const ClassEntry; 4] = [
            WasmException::get_metadata().ce(),
            CompileError::get_metadata().ce(),
            LinkError::get_metadata().ce(),
            RuntimeError::get_metadata().ce(),
        ];
        // SAFETY: internal class entries live for the whole process, no PHP code
        // has run yet in this request, and `ONCE` makes this the only writer.
        unsafe {
            for class in classes {
                let class = class.cast_mut();
                (*class).__bindgen_anon_2.create_object = (*base).__bindgen_anon_2.create_object;
                (*class).ce_flags &= !ClassFlags::NotSerializable.bits();
                // Overwriting the function in place covers `new`, `parent::__construct()`
                // and reflection, which all resolve to this same function entry.
                if let (Some(ours), Some(theirs)) = ((*class).constructor.as_mut(), (*base).constructor.as_ref()) {
                    ours.internal_function = theirs.internal_function;
                }
            }
        }
    });
}

pub fn compile_error(err: impl Display) -> PhpException {
    PhpException::from_class::<CompileError>(format!("{err:#}"))
}

pub fn link_error(err: impl Display) -> PhpException {
    PhpException::from_class::<LinkError>(format!("{err:#}"))
}

/// Formats a trap as "cause" followed by the wasm backtrace, instead of
/// wasmtime's default "error while executing at wasm backtrace: ... cause".
pub fn runtime_error(err: wasmtime::Error) -> PhpException {
    let mut message = err.root_cause().to_string();
    if let Some(backtrace) = err.downcast_ref::<wasmtime::WasmBacktrace>() {
        message.push_str(&format!("\n{backtrace}"));
    }
    PhpException::from_class::<RuntimeError>(message)
}

pub fn type_error(message: impl Into<String>) -> PhpException {
    PhpException::new(message.into(), 0, ce::type_error())
}

pub fn value_error(message: impl Into<String>) -> PhpException {
    PhpException::new(message.into(), 0, ce::value_error())
}

pub fn argument_count_error(message: impl Into<String>) -> PhpException {
    PhpException::new(message.into(), 0, ce::argument_count_error())
}

pub fn error(message: impl Into<String>) -> PhpException {
    PhpException::new(message.into(), 0, ce::error())
}
