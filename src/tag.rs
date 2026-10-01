use std::cell::Cell;
use std::rc::Rc;

use ext_php_rs::boxed::ZBox;
use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::{PhpException, PhpResult};
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, ZendObject, Zval};
use wasmtime::{FuncType, StoreContextMut, TagType, ValType};

use crate::engine::engine;
use crate::error::{type_error, value_error};
use crate::store::{self, HostState, KnownTag, SharedStore, StoreObject};
use crate::types::tag_type;
use crate::value::{debug_type, downcast, parse_val_type};

/// An exception tag, like JS `WebAssembly.Tag`.
#[php_class]
#[php(name = "Wasm\\Tag")]
#[php(flags = ClassFlags::Final)]
pub struct Tag {
    pub store: SharedStore,
    pub inner: wasmtime::Tag,
    alive: Rc<Cell<bool>>,
}

impl Drop for Tag {
    fn drop(&mut self) {
        self.alive.set(false);
    }
}

#[php_impl]
impl Tag {
    /// @param array{parameters: list<string>} $descriptor
    pub fn __construct(descriptor: &ZendHashTable, store: Option<&StoreObject>) -> PhpResult<Self> {
        let parameters = descriptor
            .get("parameters")
            .and_then(Zval::array)
            .ok_or_else(|| type_error("descriptor \"parameters\" must be a list of value types"))?;
        let parameters = parameters
            .values()
            .map(|ty| {
                ty.str()
                    .ok_or_else(|| {
                        type_error(format!(
                            "expected a value type name, got {}",
                            debug_type(ty)
                        ))
                    })
                    .and_then(parse_val_type)
            })
            .collect::<PhpResult<Vec<ValType>>>()?;
        let ty = TagType::new(FuncType::new(engine(), parameters, []));
        let store = store::choose(store, [], store::standalone)?;
        let inner = store
            .with(|mut ctx| wasmtime::Tag::new(&mut ctx, &ty))
            .map_err(|err| value_error(format!("{err:#}")))?;
        Ok(Self {
            store,
            inner,
            alive: Rc::new(Cell::new(true)),
        })
    }

    /// @return array{parameters: list<string>}
    pub fn r#type(&self) -> PhpResult<ZBox<ZendHashTable>> {
        self.store.with(|ctx| tag_type(&self.inner.ty(&ctx)))
    }
}

/// Returns the PHP object for `tag`, reusing the one PHP already knows so that
/// tags compare with `===` as they do in JS.
pub fn tag_to_zval(
    ctx: &mut StoreContextMut<'_, HostState>,
    tag: &wasmtime::Tag,
) -> Result<Zval, PhpException> {
    if let Some(object) = known_object(ctx, tag) {
        let mut zval = Zval::new();
        // SAFETY: `known_object` only returns objects whose `alive` flag is
        // still set, which their Drop clears before the object is freed.
        zval.set_object(unsafe { &mut *object });
        return Ok(zval);
    }
    let alive = Rc::new(Cell::new(true));
    let object = Tag {
        store: store::of(ctx),
        inner: *tag,
        alive: alive.clone(),
    }
    .into_zval(false)?;
    register(ctx, tag, &object, alive);
    Ok(object)
}

/// Remembers the PHP object of a tag that PHP handed to wasm.
pub fn remember_tag(ctx: &mut StoreContextMut<'_, HostState>, tag: &wasmtime::Tag, object: &Zval) {
    if known_object(ctx, tag).is_none()
        && let Some(php_tag) = downcast::<Tag>(object)
    {
        register(ctx, tag, object, php_tag.alive.clone());
    }
}

fn known_object(
    ctx: &mut StoreContextMut<'_, HostState>,
    tag: &wasmtime::Tag,
) -> Option<*mut ZendObject> {
    ctx.data_mut().tags.retain(|known| known.alive.get());
    ctx.data()
        .tags
        .iter()
        .find(|known| wasmtime::Tag::eq(&known.tag, tag, &*ctx))
        .map(|known| known.object)
}

fn register(
    ctx: &mut StoreContextMut<'_, HostState>,
    tag: &wasmtime::Tag,
    object: &Zval,
    alive: Rc<Cell<bool>>,
) {
    if let Some(object) = object.object() {
        ctx.data_mut().tags.push(KnownTag {
            tag: *tag,
            object: std::ptr::from_ref(object).cast_mut(),
            alive,
        });
    }
}
