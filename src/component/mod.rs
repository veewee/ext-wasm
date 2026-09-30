//! WebAssembly components, the typed counterpart of core modules.

pub mod error;
pub mod exports;
pub mod func;
pub mod host_resource;
pub mod http;
pub mod imports;
pub mod instance;
pub mod resource;
pub mod value;

use ext_php_rs::binary_slice::BinarySlice;
use ext_php_rs::boxed::ZBox;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::ZendHashTable;
use wasmtime::component::types::{ComponentExtern, ComponentFunc, ComponentItem, Type};

use crate::engine::{compile_in_process_pool, engine};
use crate::error::compile_error;
use crate::module::read_local_file;

/// A compiled WebAssembly component.
///
/// Compile once and instantiate as often as needed, like `Wasm\Module`.
#[php_class]
#[php(name = "Wasm\\Component\\Component")]
#[php(flags = ClassFlags::Final)]
pub struct Component {
    pub inner: wasmtime::component::Component,
}

#[php_impl]
impl Component {
    /// Compiles a component binary or WAT text.
    pub fn __construct(bytes: BinarySlice<u8>) -> PhpResult<Self> {
        Self::compile(&bytes)
    }

    /// Compiles a component file, like `new Component(file_get_contents($path))`.
    ///
    /// Reads local files only and honours open_basedir.
    pub fn from_file(path: String) -> PhpResult<Self> {
        Self::compile(&read_local_file(&path)?)
    }

    /// @return list<array{name: string, kind: string, type?: string, functions?: list<array{name: string, kind: string, type?: string}>}>
    pub fn exports(&self) -> PhpResult<ZBox<ZendHashTable>> {
        describe_all(self.inner.component_type().exports(engine()))
    }

    /// @return list<array{name: string, kind: string, type?: string, functions?: list<array{name: string, kind: string, type?: string}>}>
    pub fn imports(&self) -> PhpResult<ZBox<ZendHashTable>> {
        describe_all(self.inner.component_type().imports(engine()))
    }
}

impl Component {
    pub fn compile(bytes: &[u8]) -> PhpResult<Self> {
        let binary = wat::parse_bytes(bytes).map_err(compile_error)?;
        let inner = compile_in_process_pool(|| {
            wasmtime::component::Component::from_binary(engine(), &binary)
        })
        .map_err(compile_error)?
        .map_err(|err| {
            if wasmparser::Parser::is_core_wasm(&binary) {
                compile_error(format!("{err:#} (this is a core module, use Wasm\\Module)"))
            } else {
                compile_error(err)
            }
        })?;
        Ok(Self { inner })
    }
}

pub fn validate(binary: &[u8]) -> bool {
    wasmparser::Parser::is_component(binary)
        && compile_in_process_pool(|| wasmtime::component::Component::from_binary(engine(), binary))
            .is_ok_and(|compiled| compiled.is_ok())
}

fn describe_all<'a>(
    items: impl Iterator<Item = (&'a str, ComponentExtern<'a>)>,
) -> PhpResult<ZBox<ZendHashTable>> {
    let mut list = ZendHashTable::new();
    for (name, item) in items {
        list.push(describe(name, &item.ty)?)?;
    }
    Ok(list)
}

fn describe(name: &str, item: &ComponentItem) -> PhpResult<ZBox<ZendHashTable>> {
    let mut entry = ZendHashTable::new();
    entry.insert("name", name)?;
    match item {
        ComponentItem::ComponentFunc(func) => {
            entry.insert("kind", "function")?;
            entry.insert("type", wit_signature(func))?;
        }
        ComponentItem::ComponentInstance(instance) => {
            entry.insert("kind", "instance")?;
            entry.insert("functions", describe_all(instance.exports(engine()))?)?;
        }
        ComponentItem::CoreFunc(_) => entry.insert("kind", "core function")?,
        ComponentItem::Module(_) => entry.insert("kind", "module")?,
        ComponentItem::Component(_) => entry.insert("kind", "component")?,
        ComponentItem::Type(_) => entry.insert("kind", "type")?,
        ComponentItem::Resource(_) => entry.insert("kind", "resource")?,
    }
    Ok(entry)
}

/// A function type as WIT text, like `func(a: u32) -> string`.
///
/// wasmtime's types are structural, so named WIT types print as their structure.
pub fn wit_signature(func: &ComponentFunc) -> String {
    let params: Vec<String> = func
        .params()
        .map(|(name, ty)| format!("{name}: {}", wit_type(&ty)))
        .collect();
    let mut signature = format!("func({})", params.join(", "));
    let results: Vec<String> = func.results().map(|ty| wit_type(&ty)).collect();
    if !results.is_empty() {
        signature.push_str(&format!(" -> {}", results.join(", ")));
    }
    signature
}

pub fn wit_type(ty: &Type) -> String {
    let list = |types: Vec<String>| types.join(", ");
    match ty {
        Type::Bool => "bool".into(),
        Type::S8 => "s8".into(),
        Type::U8 => "u8".into(),
        Type::S16 => "s16".into(),
        Type::U16 => "u16".into(),
        Type::S32 => "s32".into(),
        Type::U32 => "u32".into(),
        Type::S64 => "s64".into(),
        Type::U64 => "u64".into(),
        Type::Float32 => "f32".into(),
        Type::Float64 => "f64".into(),
        Type::Char => "char".into(),
        Type::String => "string".into(),
        Type::List(inner) => format!("list<{}>", wit_type(&inner.ty())),
        Type::FixedLengthList(inner) => {
            format!("list<{}, {}>", wit_type(&inner.ty()), inner.len())
        }
        Type::Map(map) => format!("map<{}, {}>", wit_type(&map.key()), wit_type(&map.value())),
        Type::Record(record) => format!(
            "record {{ {} }}",
            list(
                record
                    .fields()
                    .map(|field| format!("{}: {}", field.name, wit_type(&field.ty)))
                    .collect()
            )
        ),
        Type::Tuple(tuple) => format!(
            "tuple<{}>",
            list(tuple.types().map(|ty| wit_type(&ty)).collect())
        ),
        Type::Variant(variant) => format!(
            "variant {{ {} }}",
            list(
                variant
                    .cases()
                    .map(|case| match case.ty {
                        Some(ty) => format!("{}({})", case.name, wit_type(&ty)),
                        None => case.name.to_string(),
                    })
                    .collect()
            )
        ),
        Type::Enum(cases) => format!(
            "enum {{ {} }}",
            list(cases.names().map(str::to_string).collect())
        ),
        Type::Flags(flags) => format!(
            "flags {{ {} }}",
            list(flags.names().map(str::to_string).collect())
        ),
        Type::Option(inner) => format!("option<{}>", wit_type(&inner.ty())),
        Type::Result(result) => match (result.ok(), result.err()) {
            (None, None) => "result".into(),
            (Some(ok), None) => format!("result<{}>", wit_type(&ok)),
            (None, Some(err)) => format!("result<_, {}>", wit_type(&err)),
            (Some(ok), Some(err)) => format!("result<{}, {}>", wit_type(&ok), wit_type(&err)),
        },
        Type::Own(_) => "own<resource>".into(),
        Type::Borrow(_) => "borrow<resource>".into(),
        Type::Future(inner) => match inner.ty() {
            Some(ty) => format!("future<{}>", wit_type(&ty)),
            None => "future".into(),
        },
        Type::Stream(inner) => match inner.ty() {
            Some(ty) => format!("stream<{}>", wit_type(&ty)),
            None => "stream".into(),
        },
        Type::ErrorContext => "error-context".into(),
    }
}
