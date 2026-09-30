use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::{PhpException, PhpResult};
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::{FuncType, StoreContextMut, TagType, ValType};

use crate::engine::engine;
use crate::error::{type_error, value_error};
use crate::store::{self, HostState, SharedStore};
use crate::value::{debug_type, parse_val_type};

/// An exception tag, like JS `WebAssembly.Tag`.
#[php_class]
#[php(name = "Wasm\\Tag")]
#[php(flags = ClassFlags::Final)]
pub struct Tag {
    pub store: SharedStore,
    pub inner: wasmtime::Tag,
}

#[php_impl]
impl Tag {
    /// @param array{parameters: list<string>} $descriptor
    pub fn __construct(descriptor: &ZendHashTable) -> PhpResult<Self> {
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
        let store = store::current();
        let inner = store
            .with(|mut ctx| wasmtime::Tag::new(&mut ctx, &ty))
            .map_err(|err| value_error(format!("{err:#}")))?;
        Ok(Self { store, inner })
    }
}

/// Returns the PHP object for `tag`, reusing the one PHP already knows so that
/// tags compare with `===` as they do in JS.
pub fn tag_to_zval(
    ctx: &mut StoreContextMut<'_, HostState>,
    tag: &wasmtime::Tag,
) -> Result<Zval, PhpException> {
    if let Some(known) = ctx
        .data()
        .tags
        .iter()
        .find(|(known, _)| wasmtime::Tag::eq(known, tag, &*ctx))
    {
        return Ok(known.1.shallow_clone());
    }
    let object = Tag {
        store: store::current(),
        inner: *tag,
    }
    .into_zval(false)?;
    ctx.data_mut().tags.push((*tag, object.shallow_clone()));
    Ok(object)
}

/// Remembers the PHP object of a tag that PHP handed to wasm.
pub fn remember_tag(ctx: &mut StoreContextMut<'_, HostState>, tag: &wasmtime::Tag, object: &Zval) {
    if !ctx
        .data()
        .tags
        .iter()
        .any(|(known, _)| wasmtime::Tag::eq(known, tag, &*ctx))
    {
        ctx.data_mut().tags.push((*tag, object.shallow_clone()));
    }
}
