use std::fmt::Display;

use ext_php_rs::exception::PhpException;
use ext_php_rs::prelude::*;
use ext_php_rs::class::RegisteredClass;
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
