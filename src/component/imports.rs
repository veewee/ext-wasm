//! PHP callables as the imports of a component.

use ext_php_rs::convert::IntoZvalDyn;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::types::{ZendCallable, ZendHashTable, Zval};
use ext_php_rs::zend::ExecutorGlobals;
use wasmtime::StoreContextMut;
use wasmtime::component::types::{ComponentFunc, ComponentItem, Type};
use wasmtime::component::{Linker, LinkerInstance, Val};

use crate::callback::FiberSwitchBlock;
use crate::component::Component;
use crate::component::error::class_entry as component_error;
use crate::component::value::{ResultValue, camel, from_val, to_val};
use crate::component::wit_type;
use crate::engine::engine;
use crate::error::link_error;
use crate::store::{self, HostState, SharedStore};
use crate::value::{debug_type, downcast};

/// Defines every import of `component` that is not WASI from `imports`, the
/// import object keyed by interface name with or without version.
pub fn link(
    store: &SharedStore,
    linker: &mut Linker<HostState>,
    component: &Component,
    imports: Option<&ZendHashTable>,
) -> PhpResult<()> {
    for (name, item) in component.inner.component_type().imports(engine()) {
        if name.starts_with("wasi:") {
            continue;
        }
        let unversioned = name.split('@').next().unwrap_or(name);
        let value = imports
            .and_then(|imports| imports.get(name).or_else(|| imports.get(unversioned)))
            .filter(|value| !value.is_null())
            .ok_or_else(|| link_error(format!("missing import \"{name}\"")))?;
        match &item.ty {
            ComponentItem::ComponentFunc(ty) => {
                define(store, &mut linker.root(), name, name, ty, value)?;
            }
            ComponentItem::ComponentInstance(instance) => {
                let functions = value.array().ok_or_else(|| {
                    link_error(format!(
                        "import \"{name}\" expects an array of functions, got {}",
                        debug_type(value)
                    ))
                })?;
                let declared: Vec<(&str, ComponentItem)> = instance
                    .exports(engine())
                    .map(|(export, item)| (export, item.ty))
                    .collect();
                for (key, _) in functions.iter() {
                    let key = key.to_string();
                    if !declared.iter().any(|(export, _)| camel(export) == key) {
                        return Err(link_error(format!(
                            "import \"{name}\" declares no function \"{key}\""
                        )));
                    }
                }
                let mut target = linker.instance(name).map_err(link_error)?;
                for (export, item) in &declared {
                    let path = format!("{name}#{export}");
                    let ComponentItem::ComponentFunc(ty) = item else {
                        return Err(unsupported_import(&path));
                    };
                    let callable = functions
                        .get(camel(export).as_str())
                        .ok_or_else(|| link_error(format!("missing import \"{path}\"")))?;
                    define(store, &mut target, export, &path, ty, callable)?;
                }
            }
            _ => return Err(unsupported_import(name)),
        }
    }
    Ok(())
}

fn unsupported_import(name: &str) -> ext_php_rs::exception::PhpException {
    link_error(format!(
        "import \"{name}\" is not a function, which is not supported yet"
    ))
}

fn define(
    store: &SharedStore,
    target: &mut LinkerInstance<'_, HostState>,
    name: &str,
    path: &str,
    ty: &ComponentFunc,
    value: &Zval,
) -> PhpResult<()> {
    if downcast::<crate::func::Func>(value).is_some() || !value.is_callable() {
        return Err(link_error(format!(
            "import \"{path}\" expects a PHP callable, got {}",
            debug_type(value)
        )));
    }
    if let Some(unsupported) = ty
        .params()
        .map(|(_, ty)| ty)
        .chain(ty.results())
        .find_map(|ty| unsupported_part(&ty))
    {
        return Err(link_error(format!(
            "import \"{path}\" uses {unsupported}, which is not supported yet"
        )));
    }
    let key = store.with(|mut ctx| {
        ctx.data_mut()
            .values
            .insert_permanent(value.shallow_clone())
    });
    target
        .func_new(name, move |mut ctx, ty, params, results| {
            invoke(&mut ctx, key, &ty, params, results)
        })
        .map_err(link_error)
}

/// The first type inside `ty` that the value mapping cannot convert yet.
fn unsupported_part(ty: &Type) -> Option<String> {
    let nested = |types: Vec<Type>| types.iter().find_map(unsupported_part);
    match ty {
        Type::Own(_)
        | Type::Borrow(_)
        | Type::Map(_)
        | Type::FixedLengthList(_)
        | Type::Future(_)
        | Type::Stream(_)
        | Type::ErrorContext => Some(wit_type(ty)),
        Type::List(list) => unsupported_part(&list.ty()),
        Type::Option(option) => unsupported_part(&option.ty()),
        Type::Tuple(tuple) => nested(tuple.types().collect()),
        Type::Record(record) => nested(record.fields().map(|field| field.ty).collect()),
        Type::Variant(variant) => nested(variant.cases().filter_map(|case| case.ty).collect()),
        Type::Result(result) => nested(result.ok().into_iter().chain(result.err()).collect()),
        _ => None,
    }
}

fn invoke(
    ctx: &mut StoreContextMut<'_, HostState>,
    key: usize,
    ty: &ComponentFunc,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let store = store::of(ctx);
    let callable = ctx.data().values.get(key).shallow_clone();
    let args = params
        .iter()
        .zip(ty.params())
        .map(|(param, (_, ty))| from_val(param, &ty))
        .collect::<Result<Vec<Zval>, _>>()?;

    let returned = store.enter_component(ctx, || {
        let args: Vec<&dyn IntoZvalDyn> = args.iter().map(|arg| arg as &dyn IntoZvalDyn).collect();
        let _no_fiber_switch = FiberSwitchBlock::new();
        ZendCallable::new(&callable)?.try_call(args)
    });

    let mut thrown = None;
    let outcome = match (&returned, ty.results().next()) {
        (Ok(_), None) => Ok(()),
        (Ok(value), Some(Type::Result(result))) => {
            let val = if downcast::<ResultValue>(value).is_some() {
                to_val(value, &Type::Result(result))
            } else {
                ok_payload(value, result.ok()).map(|payload| Val::Result(Ok(payload)))
            };
            val.map(|val| results[0] = val).map_err(Into::into)
        }
        (Ok(value), Some(result_ty)) => to_val(value, &result_ty)
            .map(|val| results[0] = val)
            .map_err(Into::into),
        // A ComponentError returns the err of a result to the component.
        (Err(_), Some(Type::Result(result))) if pending_component_error() => {
            let error = ExecutorGlobals::take_exception().expect("a pending ComponentError");
            let payload: Zval = error
                .get_property::<&Zval>("payload")
                .map_or_else(|_| Zval::null(), Zval::shallow_clone);
            let outcome = ok_payload(&payload, result.err())
                .map(|payload| results[0] = Val::Result(Err(payload)))
                .map_err(Into::into);
            thrown = Some((error, payload));
            outcome
        }
        // Any other PHP exception stays pending in the engine while the
        // component unwinds, and the PHP entry point that started the call
        // rethrows it.
        (Err(err), _) => Err(wasmtime::Error::msg(format!("PHP callback failed: {err}"))),
    };

    // Releasing these can run PHP destructors, which may use wasm objects again.
    store.enter_component(ctx, move || {
        let _no_fiber_switch = FiberSwitchBlock::new();
        drop((returned, args, callable, thrown));
    });
    outcome
}

fn pending_component_error() -> bool {
    ExecutorGlobals::get()
        .exception()
        .is_some_and(|exception| exception.instance_of(component_error()))
}

fn ok_payload(
    value: &Zval,
    ty: Option<Type>,
) -> Result<Option<Box<Val>>, crate::value::ConvertError> {
    match ty {
        Some(ty) => Ok(Some(Box::new(to_val(value, &ty)?))),
        None => Ok(None),
    }
}
