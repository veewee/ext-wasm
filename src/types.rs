//! Core wasm types as PHP arrays, shaped like the dictionaries of the JS type
//! reflection proposal (WebAssembly/js-types).

use ext_php_rs::boxed::ZBox;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::types::ZendHashTable;
use wasmtime::{
    ExternType, FuncType, GlobalType, HeapType, MemoryType, Mutability, RefType, TableType,
    TagType, ValType,
};

pub fn val_type_name(ty: &ValType) -> String {
    match ty {
        ValType::I32 => "i32".into(),
        ValType::I64 => "i64".into(),
        ValType::F32 => "f32".into(),
        ValType::F64 => "f64".into(),
        ValType::V128 => "v128".into(),
        ValType::Ref(ty) => ref_type_name(ty),
    }
}

/// Nullable abstract references by their short text-format name, as JS names
/// `funcref` and `externref`; anything else in the `(ref ...)` form. A concrete
/// type is named by its kind only: the index wasmtime has for it is engine-wide,
/// not the module's, and its structure can refer to itself. RefType's Display is
/// not used, because it prints that index.
pub fn ref_type_name(ty: &RefType) -> String {
    let heap = match ty.heap_type() {
        HeapType::Extern => "extern",
        HeapType::NoExtern => "noextern",
        HeapType::Func => "func",
        HeapType::NoFunc => "nofunc",
        HeapType::Any => "any",
        HeapType::Eq => "eq",
        HeapType::I31 => "i31",
        HeapType::Array => "array",
        HeapType::Struct => "struct",
        HeapType::None => "none",
        HeapType::Exn => "exn",
        HeapType::NoExn => "noexn",
        HeapType::Cont => "cont",
        HeapType::NoCont => "nocont",
        HeapType::ConcreteFunc(_) => "(concrete func)",
        HeapType::ConcreteArray(_) => "(concrete array)",
        HeapType::ConcreteStruct(_) => "(concrete struct)",
        HeapType::ConcreteExn(_) => "(concrete exn)",
        HeapType::ConcreteCont(_) => "(concrete cont)",
    };
    if ty.is_nullable() && !heap.starts_with('(') {
        return match heap {
            "none" => "nullref".into(),
            "noextern" => "nullexternref".into(),
            "nofunc" => "nullfuncref".into(),
            "noexn" => "nullexnref".into(),
            "nocont" => "nullcontref".into(),
            other => format!("{other}ref"),
        };
    }
    let null = if ty.is_nullable() { "null " } else { "" };
    format!("(ref {null}{heap})")
}

/// A function type for messages, like `(i32, i64) -> (f32)`.
pub fn signature(ty: &FuncType) -> String {
    let list = |types: &mut dyn Iterator<Item = ValType>| {
        types
            .map(|ty| val_type_name(&ty))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "({}) -> ({})",
        list(&mut ty.params()),
        list(&mut ty.results())
    )
}

pub fn extern_type(ty: &ExternType) -> PhpResult<ZBox<ZendHashTable>> {
    match ty {
        ExternType::Func(ty) => func_type(ty),
        ExternType::Global(ty) => global_type(ty),
        ExternType::Memory(ty) => memory_type(ty, ty.minimum()),
        ExternType::Table(ty) => table_type(ty, ty.minimum()),
        ExternType::Tag(ty) => tag_type(ty),
    }
}

pub fn func_type(ty: &FuncType) -> PhpResult<ZBox<ZendHashTable>> {
    let mut array = ZendHashTable::new();
    array.insert("parameters", names(ty.params())?)?;
    array.insert("results", names(ty.results())?)?;
    Ok(array)
}

pub fn global_type(ty: &GlobalType) -> PhpResult<ZBox<ZendHashTable>> {
    let mut array = ZendHashTable::new();
    array.insert("value", val_type_name(ty.content()))?;
    array.insert("mutable", ty.mutability() == Mutability::Var)?;
    Ok(array)
}

/// `minimum` is passed in, because a live memory reports its current size.
pub fn memory_type(ty: &MemoryType, minimum: u64) -> PhpResult<ZBox<ZendHashTable>> {
    let mut array = ZendHashTable::new();
    limits(&mut array, minimum, ty.maximum(), ty.is_64())?;
    Ok(array)
}

/// `minimum` is passed in, because a live table reports its current size.
pub fn table_type(ty: &TableType, minimum: u64) -> PhpResult<ZBox<ZendHashTable>> {
    let mut array = ZendHashTable::new();
    array.insert("element", ref_type_name(ty.element()))?;
    limits(&mut array, minimum, ty.maximum(), ty.is_64())?;
    Ok(array)
}

pub fn tag_type(ty: &TagType) -> PhpResult<ZBox<ZendHashTable>> {
    let mut array = ZendHashTable::new();
    array.insert("parameters", names(ty.ty().params())?)?;
    Ok(array)
}

/// A 64-bit limit beyond PHP's int range is a float, as PHP's own integer
/// overflow gives, so reflection still describes such a module.
fn limit(array: &mut ZendHashTable, key: &str, value: u64) -> PhpResult<()> {
    match i64::try_from(value) {
        Ok(value) => array.insert(key, value)?,
        Err(_) => array.insert(key, value as f64)?,
    }
    Ok(())
}

fn names(types: impl Iterator<Item = ValType>) -> PhpResult<ZBox<ZendHashTable>> {
    let mut list = ZendHashTable::new();
    for ty in types {
        list.push(val_type_name(&ty))?;
    }
    Ok(list)
}

fn limits(
    array: &mut ZendHashTable,
    minimum: u64,
    maximum: Option<u64>,
    is_64: bool,
) -> PhpResult<()> {
    limit(array, "minimum", minimum)?;
    if let Some(maximum) = maximum {
        limit(array, "maximum", maximum)?;
    }
    if is_64 {
        array.insert("address", "i64")?;
    }
    Ok(())
}
