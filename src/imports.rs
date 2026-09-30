use ext_php_rs::exception::PhpResult;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::{Extern, ExternType, ImportType, Mutability};

use crate::callback::host_func;
use crate::error::link_error;
use crate::func::Func;
use crate::global::{GlobalVar, new_global};
use crate::memory::Memory;
use crate::store::SharedStore;
use crate::suspend::Suspending;
use crate::table::Table;
use crate::tag::{Tag, remember_tag};
use crate::value::{debug_type, downcast, to_val};

/// Resolves a JS-style import object (`['env' => ['name' => $value]]`) against
/// the imports the module declares, in declaration order.
pub fn resolve(
    store: &SharedStore,
    module: &wasmtime::Module,
    imports: Option<&ZendHashTable>,
) -> PhpResult<Vec<Extern>> {
    module
        .imports()
        .map(|import| {
            let value = lookup(imports, &import)
                .ok_or_else(|| link_error(format!("missing import {}", describe(&import))))?;
            to_extern(store, &import, value)
        })
        .collect()
}

/// The stores of the wasm objects among the imports, which the instance has to join.
pub fn stores(
    module: &wasmtime::Module,
    imports: Option<&ZendHashTable>,
) -> Vec<(SharedStore, &'static str)> {
    module
        .imports()
        .filter_map(|import| lookup(imports, &import))
        .filter_map(owner)
        .collect()
}

/// Whether any import is a `Wasm\Suspending`. Checked before any import is
/// resolved, because the store has to be async before its first callback exists.
pub fn has_suspending(module: &wasmtime::Module, imports: Option<&ZendHashTable>) -> bool {
    module
        .imports()
        .filter_map(|import| lookup(imports, &import))
        .any(|value| downcast::<Suspending>(value).is_some())
}

fn lookup<'a>(imports: Option<&'a ZendHashTable>, import: &ImportType<'_>) -> Option<&'a Zval> {
    imports
        .and_then(|imports| imports.get(import.module()))
        .and_then(Zval::array)
        .and_then(|namespace| namespace.get(import.name()))
}

/// The store of a wasm object, if `value` is one.
fn owner(value: &Zval) -> Option<(SharedStore, &'static str)> {
    if let Some(global) = downcast::<GlobalVar>(value) {
        return Some((global.store.clone(), "GlobalVar"));
    }
    if let Some(memory) = downcast::<Memory>(value) {
        return Some((memory.store.clone(), "Memory"));
    }
    if let Some(table) = downcast::<Table>(value) {
        return Some((table.store.clone(), "Table"));
    }
    if let Some(tag) = downcast::<Tag>(value) {
        return Some((tag.store.clone(), "Tag"));
    }
    downcast::<Func>(value).map(|func| (func.store.clone(), "Func"))
}

fn to_extern(store: &SharedStore, import: &ImportType<'_>, value: &Zval) -> PhpResult<Extern> {
    if let Some(global) = downcast::<GlobalVar>(value) {
        return Ok(global.inner.into());
    }
    if let Some(memory) = downcast::<Memory>(value) {
        return Ok(memory.inner.into());
    }
    if let Some(table) = downcast::<Table>(value) {
        return Ok(table.inner.into());
    }
    if let Some(tag) = downcast::<Tag>(value) {
        store.with(|mut ctx| remember_tag(&mut ctx, &tag.inner, value));
        return Ok(tag.inner.into());
    }
    if let Some(func) = downcast::<Func>(value) {
        return Ok(func.inner.into());
    }
    if let Some(suspending) = downcast::<Suspending>(value) {
        return match import.ty() {
            ExternType::Func(ty) => Ok(store
                .with(|ctx| host_func(ctx, ty, &suspending.callback, true))
                .into()),
            _ => Err(mismatch(import, value)),
        };
    }
    match import.ty() {
        ExternType::Func(ty) if value.is_callable() => {
            Ok(store.with(|ctx| host_func(ctx, ty, value, false)).into())
        }
        // JS accepts a plain number for an immutable global import.
        ExternType::Global(ty) if ty.mutability() == Mutability::Const && !value.is_object() => {
            store.with(|mut ctx| {
                let val =
                    to_val(&mut ctx, value, ty.content()).map_err(|_| mismatch(import, value))?;
                Ok(new_global(&mut ctx, ty.content().clone(), Mutability::Const, val)?.into())
            })
        }
        _ => Err(mismatch(import, value)),
    }
}

fn mismatch(import: &ImportType<'_>, value: &Zval) -> ext_php_rs::exception::PhpException {
    link_error(format!(
        "import {} expects {}, got {}",
        describe(import),
        kind(&import.ty()),
        debug_type(value)
    ))
}

fn describe(import: &ImportType<'_>) -> String {
    format!("\"{}\".\"{}\"", import.module(), import.name())
}

pub fn kind(ty: &ExternType) -> &'static str {
    match ty {
        ExternType::Func(_) => "function",
        ExternType::Global(_) => "global",
        ExternType::Memory(_) => "memory",
        ExternType::Table(_) => "table",
        ExternType::Tag(_) => "tag",
    }
}
