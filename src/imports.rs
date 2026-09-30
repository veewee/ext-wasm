use ext_php_rs::exception::PhpResult;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::{Extern, ExternType, ImportType, Mutability};

use crate::callback::host_func;
use crate::error::link_error;
use crate::func::Func;
use crate::global::{new_global, GlobalVar};
use crate::memory::Memory;
use crate::table::Table;
use crate::tag::{remember_tag, Tag};
use crate::store::SharedStore;
use crate::value::{debug_type, downcast, to_val};

/// Resolves a JS-style import object (`['env' => ['name' => $value]]`) against
/// the imports the module declares, in declaration order.
pub fn resolve(store: &SharedStore, module: &wasmtime::Module, imports: Option<&ZendHashTable>) -> PhpResult<Vec<Extern>> {
    module
        .imports()
        .map(|import| {
            let value = imports
                .and_then(|imports| imports.get(import.module()))
                .and_then(Zval::array)
                .and_then(|namespace| namespace.get(import.name()))
                .ok_or_else(|| link_error(format!("missing import {}", describe(&import))))?;
            to_extern(store, &import, value)
        })
        .collect()
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
    match import.ty() {
        ExternType::Func(ty) if value.is_callable() => Ok(store.with(|ctx| host_func(ctx, ty, value)).into()),
        // JS accepts a plain number for an immutable global import.
        ExternType::Global(ty) if ty.mutability() == Mutability::Const && !value.is_object() => {
            store.with(|mut ctx| {
                let val = to_val(&mut ctx, value, ty.content()).map_err(|_| mismatch(import, value))?;
                Ok(new_global(&mut ctx, ty.content().clone(), Mutability::Const, val)?.into())
            })
        }
        _ => Err(mismatch(import, value)),
    }
}

fn mismatch(import: &ImportType<'_>, value: &Zval) -> ext_php_rs::exception::PhpException {
    link_error(format!("import {} expects {}, got {}", describe(import), kind(&import.ty()), debug_type(value)))
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
