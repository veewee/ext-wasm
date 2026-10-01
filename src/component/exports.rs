use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ArrayKey, ZendHashTable, Zval};
use ext_php_rs::zend::ce;
use wasmtime::component::types::{ComponentExtern, ComponentItem};
use wasmtime::component::{ComponentExportIndex, Instance};
use wasmtime::{AsContextMut, StoreContextMut};

use std::rc::Rc;

use crate::component::func::Func;
use crate::component::resource::{ResourceClass, ResourceMeta, is_resource_function};
use crate::component::types::{Names, function_type};
use crate::component::value::camel;
use crate::engine::engine;
use crate::error::error;
use crate::store::{HostState, SharedStore};
use crate::value::downcast;

/// The exports of a component instance, or of one interface it exports.
///
/// Functions are camelCase methods; `get()` takes any export by its WIT name,
/// with or without version.
#[php_class]
#[php(name = "Wasm\\Component\\Exports")]
#[php(flags = ClassFlags::Final)]
#[php(implements(ce = ce::aggregate, stub = "\\IteratorAggregate"))]
pub struct Exports {
    entries: Vec<(String, Zval)>,
}

impl Exports {
    /// Resolves `items`, the exports of `instance` below `parent`.
    pub fn new<'a>(
        store: &SharedStore,
        ctx: &mut StoreContextMut<'_, HostState>,
        instance: &Instance,
        parent: Option<&ComponentExportIndex>,
        items: impl Iterator<Item = (&'a str, ComponentExtern<'a>)>,
    ) -> PhpResult<Self> {
        let items: Vec<(&str, ComponentExtern<'a>)> = items.collect();
        let names = Names::of(items.iter().map(|(name, item)| (*name, item)));
        // Functions first: a resource type collects its constructor, static
        // functions and methods from them.
        let mut functions = Vec::new();
        for (name, item) in &items {
            if let ComponentItem::ComponentFunc(ty) = &item.ty
                && let Some(index) = instance.get_export_index(ctx.as_context_mut(), parent, name)
                && let Some(inner) = instance.get_func(ctx.as_context_mut(), index)
            {
                let signature = function_type(ty, &names)?.into_zval(false)?;
                functions.push((
                    name.to_string(),
                    Func {
                        store: store.clone(),
                        inner,
                        signature,
                    },
                ));
            }
        }
        let mut entries = Vec::new();
        for (name, item) in items {
            let object = match &item.ty {
                ComponentItem::ComponentFunc(_) => {
                    if is_resource_function(name) {
                        continue;
                    }
                    let Some((_, func)) = functions.iter().find(|(export, _)| export == name)
                    else {
                        continue;
                    };
                    func.clone().into_zval(false)?
                }
                ComponentItem::ComponentInstance(nested) => {
                    let Some(index) = instance.get_export_index(ctx.as_context_mut(), parent, name)
                    else {
                        continue;
                    };
                    Self::new(store, ctx, instance, Some(&index), nested.exports(engine()))?
                        .into_zval(false)?
                }
                ComponentItem::Resource(_) => {
                    // The type of the instance, which the handles it returns carry.
                    let Some((ComponentItem::Resource(ty), _)) =
                        instance.get_export(ctx.as_context_mut(), parent, name)
                    else {
                        continue;
                    };
                    let meta = Rc::new(ResourceMeta::new(name, ty, &functions));
                    store.resource_types.borrow_mut().push(meta.clone());
                    ResourceClass {
                        store: store.clone(),
                        meta,
                    }
                    .into_zval(false)?
                }
                _ => continue,
            };
            entries.push((name.to_string(), object));
        }
        Ok(Self { entries })
    }

    pub fn interface(&self, name: &str) -> Option<&Exports> {
        self.find(name).ok().and_then(downcast::<Exports>)
    }

    pub fn func(&self, name: &str) -> Option<wasmtime::component::Func> {
        self.find(name)
            .ok()
            .and_then(downcast::<Func>)
            .map(|func| func.inner)
    }

    /// The export `name`, by its WIT name with or without version.
    pub fn entry(&self, name: &str) -> Option<&Zval> {
        self.find(name).ok()
    }

    fn find(&self, name: &str) -> PhpResult<&Zval> {
        self.entries
            .iter()
            .find(|(export, _)| export == name)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|(export, _)| export.split('@').next() == Some(name))
            })
            .map(|(_, object)| object)
            .ok_or_else(|| error(format!("component has no export named \"{name}\"")))
    }
}

#[php_impl]
impl Exports {
    /// @return \Wasm\Component\Func|\Wasm\Component\Exports
    pub fn get(&self, name: String) -> PhpResult<Zval> {
        Ok(self.find(&name)?.shallow_clone())
    }

    pub fn __call(&self, name: String, arguments: &ZendHashTable) -> PhpResult<Zval> {
        let func = self
            .entries
            .iter()
            .filter_map(|(export, object)| downcast::<Func>(object).map(|func| (export, func)))
            .find(|(export, _)| camel(export) == name)
            .map(|(_, func)| func)
            .ok_or_else(|| error(format!("component has no function named \"{name}\"")))?;
        // PHP collects named arguments under their names, which say nothing
        // about the WIT parameter order.
        if arguments
            .iter()
            .any(|(key, _)| !matches!(key, ArrayKey::Long(_)))
        {
            return Err(error(
                "component functions take positional arguments, not named arguments",
            ));
        }
        let args: Vec<&Zval> = arguments.values().collect();
        func.call(&args)
    }

    /// Every export by WIT name. An aggregate rather than an Iterator, so WIT
    /// functions called next or current stay callable as methods.
    pub fn get_iterator(&self) -> ExportsIterator {
        ExportsIterator {
            entries: self
                .entries
                .iter()
                .map(|(name, object)| (name.clone(), object.shallow_clone()))
                .collect(),
            position: 0,
        }
    }
}

/// Iterates the exports of a component instance by WIT name.
#[php_class]
#[php(name = "Wasm\\Component\\ExportsIterator")]
#[php(flags = ClassFlags::Final)]
#[php(implements(ce = ce::iterator, stub = "\\Iterator"))]
pub struct ExportsIterator {
    entries: Vec<(String, Zval)>,
    position: usize,
}

#[php_impl]
impl ExportsIterator {
    /// @return \Wasm\Component\Func|\Wasm\Component\Exports|null
    pub fn current(&self) -> Zval {
        self.entries
            .get(self.position)
            .map_or_else(Zval::null, |(_, object)| object.shallow_clone())
    }

    pub fn key(&self) -> Option<String> {
        self.entries
            .get(self.position)
            .map(|(name, _)| name.clone())
    }

    pub fn next(&mut self) {
        self.position += 1;
    }

    pub fn rewind(&mut self) {
        self.position = 0;
    }

    pub fn valid(&self) -> bool {
        self.position < self.entries.len()
    }
}
