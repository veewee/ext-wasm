use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use ext_php_rs::zend::ce;
use wasmtime::Extern;

use crate::error::error;
use crate::func::{Func, call};
use crate::global::GlobalVar;
use crate::memory::Memory;
use crate::store::SharedStore;
use crate::table::Table;
use crate::tag::tag_to_zval;

/// The exports of an instance, like JS `instance.exports`.
///
/// Wrapper objects are created once, so `$exports->f === $exports->f` holds as in JS.
#[php_class]
#[php(name = "Wasm\\Exports")]
#[php(flags = ClassFlags::Final)]
#[php(implements(ce = ce::iterator, stub = "\\Iterator"))]
pub struct Exports {
    store: SharedStore,
    entries: Vec<(String, Extern, Zval)>,
    position: usize,
}

impl Exports {
    pub fn new(store: SharedStore, externs: Vec<(String, Extern)>) -> PhpResult<Self> {
        let entries = externs
            .into_iter()
            .map(|(name, ext)| {
                let object = wrap_extern(&store, ext.clone())?;
                Ok((name, ext, object))
            })
            .collect::<PhpResult<_>>()?;
        Ok(Self {
            store,
            entries,
            position: 0,
        })
    }

    fn find(&self, name: &str) -> PhpResult<&(String, Extern, Zval)> {
        self.entries
            .iter()
            .find(|(export, _, _)| export == name)
            .ok_or_else(|| error(format!("wasm module has no export named \"{name}\"")))
    }
}

#[php_impl]
impl Exports {
    pub fn __get(&self, name: String) -> PhpResult<Zval> {
        Ok(self.find(&name)?.2.shallow_clone())
    }

    pub fn __isset(&self, name: String) -> bool {
        self.find(&name).is_ok()
    }

    pub fn __call(&self, name: String, arguments: &ZendHashTable) -> PhpResult<Zval> {
        let Extern::Func(func) = &self.find(&name)?.1 else {
            return Err(error(format!("wasm export \"{name}\" is not a function")));
        };
        let args: Vec<&Zval> = arguments.values().collect();
        call(&self.store, func, &args)
    }

    pub fn current(&self) -> Zval {
        self.entries
            .get(self.position)
            .map_or_else(Zval::null, |(_, _, object)| object.shallow_clone())
    }

    pub fn key(&self) -> Option<String> {
        self.entries
            .get(self.position)
            .map(|(name, _, _)| name.clone())
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

fn wrap_extern(store: &SharedStore, ext: Extern) -> PhpResult<Zval> {
    match ext {
        Extern::Func(inner) => Ok(Func {
            store: store.clone(),
            inner,
        }
        .into_zval(false)?),
        Extern::Global(inner) => Ok(GlobalVar {
            store: store.clone(),
            inner,
        }
        .into_zval(false)?),
        Extern::Memory(inner) => Ok(Memory {
            store: store.clone(),
            inner,
        }
        .into_zval(false)?),
        Extern::Table(inner) => Ok(Table {
            store: store.clone(),
            inner,
        }
        .into_zval(false)?),
        Extern::Tag(inner) => store.with(|mut ctx| tag_to_zval(&mut ctx, &inner)),
        _ => Err(error("unsupported export kind")),
    }
}
