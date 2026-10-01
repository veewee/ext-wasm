//! Reflection of WIT types: `Wasm\Component\Type\FunctionType` and
//! `Wasm\Component\Type\ValueType`, named as the component names them.

use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::component::ResourceType;
use wasmtime::component::types::{ComponentExtern, ComponentFunc, ComponentItem, Type};

/// The names a scope (a world or an interface) gives its types and resources.
#[derive(Default)]
pub struct Names {
    types: Vec<(String, Type)>,
    resources: Vec<(String, ResourceType)>,
}

impl Names {
    pub fn of<'a>(items: impl IntoIterator<Item = (&'a str, &'a ComponentExtern<'a>)>) -> Self {
        let mut names = Self::default();
        for (name, item) in items {
            match &item.ty {
                ComponentItem::Type(ty) => names.types.push((name.to_string(), ty.clone())),
                ComponentItem::Resource(ty) => names.resources.push((name.to_string(), *ty)),
                _ => {}
            }
        }
        names
    }

    /// The name of `ty`, when exactly one named type of the scope matches it.
    ///
    /// wasmtime compares types by structure, so two names for the same shape
    /// leave the type nameless. Types that reach a resource are not compared:
    /// comparing them panics inside wasmtime for types of an uninstantiated
    /// component.
    pub fn type_name(&self, ty: &Type) -> Option<&str> {
        if reaches_resource(ty) {
            return None;
        }
        let mut matches = self
            .types
            .iter()
            .filter(|(_, named)| !reaches_resource(named) && named == ty);
        match (matches.next(), matches.next()) {
            (Some((name, _)), None) => Some(name.as_str()),
            _ => None,
        }
    }

    pub fn resource_name(&self, ty: &ResourceType) -> Option<&str> {
        self.resources
            .iter()
            .find(|(_, named)| named == ty)
            .map(|(name, _)| name.as_str())
    }
}

fn reaches_resource(ty: &Type) -> bool {
    let any = |types: Vec<Type>| types.iter().any(reaches_resource);
    match ty {
        Type::Own(_) | Type::Borrow(_) => true,
        Type::List(list) => reaches_resource(&list.ty()),
        Type::FixedLengthList(list) => reaches_resource(&list.ty()),
        Type::Map(map) => reaches_resource(&map.key()) || reaches_resource(&map.value()),
        Type::Option(option) => reaches_resource(&option.ty()),
        Type::Tuple(tuple) => any(tuple.types().collect()),
        Type::Record(record) => any(record.fields().map(|field| field.ty).collect()),
        Type::Variant(variant) => any(variant.cases().filter_map(|case| case.ty).collect()),
        Type::Result(result) => any(result.ok().into_iter().chain(result.err()).collect()),
        Type::Future(future) => future.ty().is_some_and(|ty| reaches_resource(&ty)),
        Type::Stream(stream) => stream.ty().is_some_and(|ty| reaches_resource(&ty)),
        _ => false,
    }
}

/// A WIT function type: its parameters by name and its result.
#[php_class]
#[php(name = "Wasm\\Component\\Type\\FunctionType")]
#[php(flags = ClassFlags::Final)]
pub struct FunctionType {
    params: Zval,
    result: Zval,
    text: String,
}

#[php_impl]
impl FunctionType {
    /// @return array<string, \Wasm\Component\Type\ValueType>
    #[php(getter)]
    pub fn get_params(&self) -> Zval {
        self.params.shallow_clone()
    }

    /// @return \Wasm\Component\Type\ValueType|null
    #[php(getter)]
    pub fn get_result(&self) -> Zval {
        self.result.shallow_clone()
    }

    /// The type as WIT, like `func(markdown: string) -> string`.
    pub fn __to_string(&self) -> String {
        self.text.clone()
    }
}

/// A WIT value type. `kind` is the WIT keyword; the other properties are set
/// for the kinds they belong to and null otherwise. `map`, `future`, `stream`
/// and fixed-length lists only report their kind: components using them do
/// not compile yet.
#[php_class]
#[php(name = "Wasm\\Component\\Type\\ValueType")]
#[php(flags = ClassFlags::Final)]
pub struct ValueType {
    kind: String,
    name: Option<String>,
    element: Zval,
    types: Zval,
    fields: Zval,
    cases: Zval,
    names: Zval,
    ok: Zval,
    err: Zval,
    resource: Option<String>,
}

#[php_impl]
impl ValueType {
    #[php(getter)]
    pub fn get_kind(&self) -> String {
        self.kind.clone()
    }

    /// The name the component gives the type, if any.
    #[php(getter)]
    pub fn get_name(&self) -> Option<String> {
        self.name.clone()
    }

    /// The element of a list, or the value of an option.
    ///
    /// @return \Wasm\Component\Type\ValueType|null
    #[php(getter)]
    pub fn get_element(&self) -> Zval {
        self.element.shallow_clone()
    }

    /// @return list<\Wasm\Component\Type\ValueType>|null
    #[php(getter)]
    pub fn get_types(&self) -> Zval {
        self.types.shallow_clone()
    }

    /// @return array<string, \Wasm\Component\Type\ValueType>|null
    #[php(getter)]
    pub fn get_fields(&self) -> Zval {
        self.fields.shallow_clone()
    }

    /// @return array<string, \Wasm\Component\Type\ValueType|null>|null
    #[php(getter)]
    pub fn get_cases(&self) -> Zval {
        self.cases.shallow_clone()
    }

    /// @return list<string>|null
    #[php(getter)]
    pub fn get_names(&self) -> Zval {
        self.names.shallow_clone()
    }

    /// @return \Wasm\Component\Type\ValueType|null
    #[php(getter)]
    pub fn get_ok(&self) -> Zval {
        self.ok.shallow_clone()
    }

    /// @return \Wasm\Component\Type\ValueType|null
    #[php(getter)]
    pub fn get_err(&self) -> Zval {
        self.err.shallow_clone()
    }

    /// The resource of an own or borrow handle.
    #[php(getter)]
    pub fn get_resource(&self) -> Option<String> {
        self.resource.clone()
    }
}

pub fn function_type(func: &ComponentFunc, names: &Names) -> PhpResult<FunctionType> {
    let mut params = ZendHashTable::new();
    for (name, ty) in func.params() {
        params.insert(name, value_type(&ty, names)?.into_zval(false)?)?;
    }
    let result = match func.results().next() {
        Some(ty) => value_type(&ty, names)?.into_zval(false)?,
        None => Zval::null(),
    };
    Ok(FunctionType {
        params: params.into_zval(false)?,
        result,
        text: crate::component::wit_signature(func, names),
    })
}

pub fn value_type(ty: &Type, names: &Names) -> PhpResult<ValueType> {
    let mut value = ValueType {
        kind: kind(ty).to_string(),
        name: names.type_name(ty).map(str::to_string),
        element: Zval::null(),
        types: Zval::null(),
        fields: Zval::null(),
        cases: Zval::null(),
        names: Zval::null(),
        ok: Zval::null(),
        err: Zval::null(),
        resource: None,
    };
    let nested = |ty: &Type| -> PhpResult<Zval> { Ok(value_type(ty, names)?.into_zval(false)?) };
    let optional = |ty: Option<Type>| -> PhpResult<Zval> {
        ty.map_or_else(|| Ok(Zval::null()), |ty| nested(&ty))
    };
    match ty {
        Type::List(list) => value.element = nested(&list.ty())?,
        Type::Option(option) => value.element = nested(&option.ty())?,
        Type::Tuple(tuple) => {
            let mut types = ZendHashTable::new();
            for ty in tuple.types() {
                types.push(nested(&ty)?)?;
            }
            value.types = types.into_zval(false)?;
        }
        Type::Record(record) => {
            let mut fields = ZendHashTable::new();
            for field in record.fields() {
                fields.insert(field.name, nested(&field.ty)?)?;
            }
            value.fields = fields.into_zval(false)?;
        }
        Type::Variant(variant) => {
            let mut cases = ZendHashTable::new();
            for case in variant.cases() {
                cases.insert(case.name, optional(case.ty)?)?;
            }
            value.cases = cases.into_zval(false)?;
        }
        Type::Enum(cases) => value.names = string_list(cases.names())?,
        Type::Flags(flags) => value.names = string_list(flags.names())?,
        Type::Result(result) => {
            value.ok = optional(result.ok())?;
            value.err = optional(result.err())?;
        }
        Type::Own(resource) | Type::Borrow(resource) => {
            value.resource = names.resource_name(resource).map(str::to_string);
        }
        _ => {}
    }
    Ok(value)
}

fn string_list<'a>(items: impl Iterator<Item = &'a str>) -> PhpResult<Zval> {
    let mut list = ZendHashTable::new();
    for item in items {
        list.push(item)?;
    }
    Ok(list.into_zval(false)?)
}

fn kind(ty: &Type) -> &'static str {
    match ty {
        Type::Bool => "bool",
        Type::S8 => "s8",
        Type::U8 => "u8",
        Type::S16 => "s16",
        Type::U16 => "u16",
        Type::S32 => "s32",
        Type::U32 => "u32",
        Type::S64 => "s64",
        Type::U64 => "u64",
        Type::Float32 => "f32",
        Type::Float64 => "f64",
        Type::Char => "char",
        Type::String => "string",
        Type::List(_) | Type::FixedLengthList(_) => "list",
        Type::Map(_) => "map",
        Type::Record(_) => "record",
        Type::Tuple(_) => "tuple",
        Type::Variant(_) => "variant",
        Type::Enum(_) => "enum",
        Type::Option(_) => "option",
        Type::Result(_) => "result",
        Type::Flags(_) => "flags",
        Type::Own(_) => "own",
        Type::Borrow(_) => "borrow",
        Type::Future(_) => "future",
        Type::Stream(_) => "stream",
        Type::ErrorContext => "error-context",
    }
}
