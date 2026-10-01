//! PHP callables as the imports of a component.

use ext_php_rs::convert::IntoZval;
use ext_php_rs::convert::IntoZvalDyn;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::types::{ZendCallable, ZendHashTable, ZendObject, Zval};
use ext_php_rs::zend::ClassEntry;
use ext_php_rs::zend::ExecutorGlobals;
use wasmtime::AsContextMut;
use wasmtime::StoreContextMut;
use wasmtime::component::types::{ComponentFunc, ComponentItem, Type};
use wasmtime::component::{Accessor, Linker, LinkerInstance, Val};

use crate::callback::FiberSwitchBlock;
use crate::component::Component;
use crate::component::error::class_entry as component_error;
use std::rc::Rc;

use ext_php_rs::error::Error;

use crate::component::exports::Exports;
use crate::component::host_resource::{self, HostImpl};
use crate::component::resource::{
    self, Resource, ResourceClass, ResourceMeta, is_resource_function,
};
use crate::component::value::{ResultValue, camel, from_val, to_val};
use crate::component::wit_type;
use crate::engine::engine;
use crate::error::link_error;
use crate::store::{self, Active, HostState, SharedStore};
use crate::suspend::{Callee, Request, Suspending};
use crate::value::{debug_type, downcast};

/// Defines every import of `component` that is not WASI from `imports`, the
/// import object keyed by interface name with or without version.
pub fn link(
    store: &SharedStore,
    linker: &mut Linker<HostState>,
    component: &Component,
    imports: Option<&ZendHashTable>,
) -> PhpResult<()> {
    let is_async = store.with(|ctx| ctx.data().is_async);
    for (name, item) in component.inner.component_type().imports(engine()) {
        if name.starts_with("wasi:") {
            continue;
        }
        let unversioned = name.split('@').next().unwrap_or(name);
        if matches!(item.ty, ComponentItem::Resource(_)) {
            // Without a class this is a world's `use` of an interface resource,
            // which the interface import defines.
            let Some(value) =
                imports.and_then(|imports| imports.get(name).or_else(|| imports.get(unversioned)))
            else {
                continue;
            };
            let (implementation, _) = resource_impl(store, Some(value), name)?;
            let ty = store.with(|mut ctx| ctx.data_mut().host_resources.register(implementation));
            linker
                .root()
                .resource(name, ty, release_host_resource)
                .map_err(link_error)?;
            continue;
        }
        let value = imports
            .and_then(|imports| imports.get(name).or_else(|| imports.get(unversioned)))
            .filter(|value| !value.is_null())
            .ok_or_else(|| link_error(format!("missing import \"{name}\"")))?;
        match &item.ty {
            ComponentItem::ComponentFunc(ty) => {
                let target = callable_target(store, value, name)?;
                define(&mut linker.root(), name, name, ty, target, is_async)?;
            }
            ComponentItem::ComponentInstance(instance) => {
                let provided = match (value.array(), downcast::<Exports>(value)) {
                    (Some(functions), _) => Provided::Array(functions),
                    (_, Some(exports)) => Provided::Exports(exports),
                    _ => {
                        return Err(link_error(format!(
                            "import \"{name}\" expects an array of functions or an exported interface, got {}",
                            debug_type(value)
                        )));
                    }
                };
                let declared: Vec<(&str, ComponentItem)> = instance
                    .exports(engine())
                    .map(|(export, item)| (export, item.ty))
                    .collect();
                // Keys PHP may give: the resources by name, the functions camelCase.
                for (key, _) in provided.keys() {
                    let key = key.to_string();
                    let known = declared.iter().any(|(export, item)| match item {
                        ComponentItem::Resource(_) => *export == key,
                        _ => !is_resource_function(export) && camel(export) == key,
                    });
                    if !known {
                        return Err(link_error(format!(
                            "import \"{name}\" declares no function \"{key}\""
                        )));
                    }
                }
                let mut target = linker.instance(name).map_err(link_error)?;
                let mut classes: Vec<(String, HostImpl, Option<Rc<ResourceMeta>>)> = Vec::new();
                for (export, item) in &declared {
                    if let ComponentItem::Resource(_) = item {
                        if let Provided::Exports(_) = provided
                            && provided.get(export).is_none()
                        {
                            return Err(link_error(format!(
                                "the exported interface given for \"{name}\" has no resource \"{export}\""
                            )));
                        }
                        let (implementation, meta) = resource_impl(
                            store,
                            provided.get(export),
                            &format!("{name}#{export}"),
                        )?;
                        let ty = store.with(|mut ctx| {
                            ctx.data_mut()
                                .host_resources
                                .register(implementation.clone())
                        });
                        target
                            .resource(export, ty, release_host_resource)
                            .map_err(link_error)?;
                        classes.push((export.to_string(), implementation, meta));
                    }
                }
                for (export, item) in &declared {
                    let path = format!("{name}#{export}");
                    let ComponentItem::ComponentFunc(ty) = item else {
                        continue;
                    };
                    let target_fn = if is_resource_function(export) {
                        resource_target(export, &classes, &path)?
                    } else {
                        let callable = provided
                            .function(export)
                            .ok_or_else(|| link_error(format!("missing import \"{path}\"")))?;
                        callable_target(store, callable, &path)?
                    };
                    define(&mut target, export, &path, ty, target_fn, is_async)?;
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

/// The value PHP gave for an interface import: an array keyed by camelCase
/// function and resource name, or the interface another instance exports.
enum Provided<'a> {
    Array(&'a ZendHashTable),
    Exports(&'a Exports),
}

impl Provided<'_> {
    /// The keys to check against the interface; an exported interface may
    /// offer more than the import declares.
    fn keys(&self) -> Vec<(String, ())> {
        match self {
            Self::Array(functions) => functions
                .iter()
                .map(|(key, _)| (key.to_string(), ()))
                .collect(),
            Self::Exports(_) => Vec::new(),
        }
    }

    fn get(&self, resource: &str) -> Option<&Zval> {
        match self {
            Self::Array(functions) => functions.get(resource),
            Self::Exports(exports) => exports.entry(resource),
        }
    }

    fn function(&self, export: &str) -> Option<&Zval> {
        match self {
            Self::Array(functions) => functions.get(camel(export).as_str()),
            Self::Exports(exports) => exports.entry(export),
        }
    }
}

/// What a component import runs in PHP.
#[derive(Clone)]
pub enum Target {
    /// A callable, by its key in the store's values, and whether it was
    /// given as a `Wasm\Suspending`, which may switch Fibers.
    Callable(usize, bool),
    /// `new Class(...)` for `[constructor]resource`.
    Constructor(String),
    /// `ResourceClass::new(...)` of another instance, by its key in the store's values.
    ComponentConstructor(usize),
    /// An instance method for `[method]resource.name`, by WIT name; the
    /// first argument is the object or the other instance's handle.
    Method(String),
    /// A static method `Class::name` for `[static]resource.name`.
    Static(String),
    /// A static function of another instance's `ResourceClass`, by its key
    /// in the store's values and camelCase name.
    ComponentStatic(usize, String),
}

fn callable_target(store: &SharedStore, value: &Zval, path: &str) -> PhpResult<Target> {
    if let Some(suspending) = downcast::<Suspending>(value) {
        let key = store.with(|mut ctx| {
            ctx.data_mut()
                .values
                .insert_permanent(suspending.callback.shallow_clone())
        });
        return Ok(Target::Callable(key, true));
    }
    if downcast::<crate::func::Func>(value).is_some() || !value.is_callable() {
        return Err(link_error(format!(
            "import \"{path}\" expects a PHP callable, got {}",
            debug_type(value)
        )));
    }
    let key = store.with(|mut ctx| {
        ctx.data_mut()
            .values
            .insert_permanent(value.shallow_clone())
    });
    Ok(Target::Callable(key, false))
}

/// The destructor of a host resource: the component dropped its handle. The
/// object goes where PHP destructors may run, not inside this call.
fn release_host_resource(
    mut ctx: StoreContextMut<'_, HostState>,
    rep: u32,
) -> wasmtime::Result<()> {
    if let Some(object) = ctx.data_mut().host_resources.take(rep) {
        store::of(&ctx).put_garbage(object);
    }
    Ok(())
}

/// What implements a resource import: another instance's `ResourceClass`,
/// with what it offers, or a PHP class.
fn resource_impl(
    store: &SharedStore,
    value: Option<&Zval>,
    path: &str,
) -> PhpResult<(HostImpl, Option<Rc<ResourceMeta>>)> {
    if let Some(class) = value.and_then(downcast::<ResourceClass>) {
        let meta = class.meta.clone();
        let key = store.with(|mut ctx| {
            ctx.data_mut()
                .values
                .insert_permanent(value.map_or_else(Zval::null, Zval::shallow_clone))
        });
        return Ok((HostImpl::Component(key), Some(meta)));
    }
    Ok((HostImpl::Class(resource_class(value, path)?), None))
}

fn resource_class(value: Option<&Zval>, path: &str) -> PhpResult<String> {
    let class = value
        .and_then(|value| value.str().filter(|_| value.is_string()))
        .filter(|class| ClassEntry::try_find(class).is_some())
        .ok_or_else(|| {
            link_error(format!(
                "import \"{path}\" is a resource and expects the name of a PHP class implementing it or a Wasm\\Component\\ResourceClass, got {}",
                value.map_or_else(|| "nothing".to_string(), debug_type)
            ))
        })?;
    let not_instantiable = ext_php_rs::ffi::ZEND_ACC_INTERFACE
        | ext_php_rs::ffi::ZEND_ACC_TRAIT
        | ext_php_rs::ffi::ZEND_ACC_IMPLICIT_ABSTRACT_CLASS
        // ZEND_ACC_EXPLICIT_ABSTRACT_CLASS, which the bindings leave out.
        | (1 << 6)
        | ext_php_rs::ffi::ZEND_ACC_ENUM;
    if ClassEntry::try_find(class).is_some_and(|entry| entry.ce_flags & not_instantiable != 0) {
        return Err(link_error(format!(
            "import \"{path}\" needs an instantiable class, {class} is abstract, an interface, a trait or an enum"
        )));
    }
    Ok(class.to_string())
}

/// The PHP method behind a `[constructor]`, `[method]` or `[static]` import.
fn resource_target(
    export: &str,
    classes: &[(String, HostImpl, Option<Rc<ResourceMeta>>)],
    path: &str,
) -> PhpResult<Target> {
    let impl_of = |resource: &str| {
        classes
            .iter()
            .find(|(name, ..)| name == resource)
            .map(|(_, implementation, meta)| (implementation.clone(), meta.clone()))
            .ok_or_else(|| {
                link_error(format!(
                    "import \"{path}\" belongs to no resource of this interface"
                ))
            })
    };
    let lacks = |meta: &ResourceMeta, what: &str| {
        link_error(format!(
            "the resource \"{}\" given for \"{path}\" has no {what}",
            meta.name
        ))
    };
    if let Some(resource) = export.strip_prefix("[constructor]") {
        return match impl_of(resource)? {
            (HostImpl::Component(key), Some(meta)) => match meta.constructor {
                Some(_) => Ok(Target::ComponentConstructor(key)),
                None => Err(lacks(&meta, "constructor")),
            },
            (HostImpl::Class(class), _) => Ok(Target::Constructor(class)),
            (HostImpl::Component(_), None) => unreachable!("a resource class has its meta"),
        };
    }
    let (kind, rest) = if let Some(rest) = export.strip_prefix("[method]") {
        ("method", rest)
    } else {
        ("static", export.strip_prefix("[static]").unwrap_or(export))
    };
    let (resource, function) = rest
        .split_once('.')
        .ok_or_else(|| link_error(format!("import \"{path}\" has no resource prefix")))?;
    let method = camel(function);
    let class = match impl_of(resource)? {
        (HostImpl::Component(key), Some(meta)) => {
            return if kind == "method" {
                match meta.methods.iter().any(|(name, _)| name == function) {
                    true => Ok(Target::Method(function.to_string())),
                    false => Err(lacks(&meta, &format!("method \"{function}\""))),
                }
            } else {
                match meta.statics.iter().any(|(name, _)| *name == method) {
                    true => Ok(Target::ComponentStatic(key, method)),
                    false => Err(lacks(&meta, &format!("static function \"{function}\""))),
                }
            };
        }
        (HostImpl::Class(class), _) => class,
        (HostImpl::Component(_), None) => unreachable!("a resource class has its meta"),
    };
    let is_static = php_bool("is_callable", &[&format!("{class}::{method}")]);
    let exists = php_bool("method_exists", &[&class, &method]);
    match kind {
        "method" if exists && !is_static => Ok(Target::Method(function.to_string())),
        "static" if is_static => Ok(Target::Static(format!("{class}::{method}"))),
        _ => Err(link_error(format!(
            "{class} implements \"{path}\" and needs a public {} method {method}()",
            if kind == "static" {
                "static"
            } else {
                "instance"
            }
        ))),
    }
}

fn php_bool(function: &str, args: &[&String]) -> bool {
    let args: Vec<&dyn IntoZvalDyn> = args.iter().map(|arg| *arg as &dyn IntoZvalDyn).collect();
    ZendCallable::try_from_name(function)
        .ok()
        .and_then(|callable| callable.try_call(args).ok())
        .and_then(|result| result.bool())
        .unwrap_or(false)
}

fn define(
    target: &mut LinkerInstance<'_, HostState>,
    name: &str,
    path: &str,
    ty: &ComponentFunc,
    what: Target,
    is_async: bool,
) -> PhpResult<()> {
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
    if ty.async_() {
        // An `async func` import must be concurrent, which wasmtime checks.
        return target
            .func_new_concurrent(name, move |accessor, ty, params, results| {
                Box::pin(ConcurrentHostCall {
                    accessor,
                    ty,
                    params,
                    results,
                    what: what.clone(),
                    id: None,
                })
            })
            .map_err(link_error);
    }
    if is_async {
        // PHP code cannot run on wasmtime's async stack, so the call is handed
        // to the poll loop on the PHP stack, as for core Suspending imports.
        return target
            .func_new_async(name, move |ctx, ty, params, results| {
                Box::new(ComponentHostCall {
                    ctx,
                    ty,
                    params,
                    results,
                    what: what.clone(),
                    id: None,
                })
            })
            .map_err(link_error);
    }
    target
        .func_new(name, move |mut ctx, ty, params, results| {
            invoke(&mut ctx, &what, &ty, params, results)
        })
        .map_err(link_error)
}

/// The first type inside `ty` that the value mapping cannot convert yet.
fn unsupported_part(ty: &Type) -> Option<String> {
    let nested = |types: Vec<Type>| types.iter().find_map(unsupported_part);
    match ty {
        Type::Map(map) if !crate::component::value::is_map_key(&map.key()) => Some(wit_type(ty)),
        Type::Map(map) => unsupported_part(&map.value()),
        Type::FixedLengthList(_) | Type::ErrorContext => Some(wit_type(ty)),
        Type::Stream(stream) if !crate::component::stream::supports(stream.ty().as_ref()) => {
            Some(wit_type(ty))
        }
        Type::Future(future) if !crate::component::stream::supports(future.ty().as_ref()) => {
            Some(wit_type(ty))
        }
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
    what: &Target,
    ty: &ComponentFunc,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let store = store::of(ctx);
    let callable = callable_of(ctx, what);
    let args = lift(ctx, &store, ty, params)?;
    let returned = store.enter_component(ctx, || {
        let _no_fiber_switch = FiberSwitchBlock::new();
        call_target(what, &callable, &args)
    });
    let (outcome, leftovers) = settle(ctx, returned.map_err(|err| err.to_string()), ty, results);
    // Releasing these can run PHP destructors, which may use wasm objects again.
    store.enter_component(ctx, move || {
        let _no_fiber_switch = FiberSwitchBlock::new();
        drop((leftovers, args, callable));
    });
    outcome
}

fn callable_of(ctx: &StoreContextMut<'_, HostState>, what: &Target) -> Zval {
    match what {
        Target::Callable(key, _)
        | Target::ComponentConstructor(key)
        | Target::ComponentStatic(key, _) => ctx.data().values.get(*key).shallow_clone(),
        _ => Zval::null(),
    }
}

/// The parameters of an import call as PHP values.
fn lift(
    ctx: &mut StoreContextMut<'_, HostState>,
    store: &SharedStore,
    ty: &ComponentFunc,
    params: &[Val],
) -> wasmtime::Result<Vec<Zval>> {
    let mut args = Vec::with_capacity(params.len());
    for (param, (_, param_ty)) in params.iter().zip(ty.params()) {
        match from_val(ctx, param, &param_ty) {
            Ok(arg) => args.push(arg),
            Err(err) => {
                // Resources lifted so far are PHP objects now; they are
                // released where their destructors may run.
                for arg in args {
                    store.put_garbage(arg);
                }
                return Err(err.into());
            }
        }
    }
    Ok(args)
}

/// Gives the component what the PHP call returned or threw, and hands back
/// the PHP values the caller must release.
fn settle(
    ctx: &mut StoreContextMut<'_, HostState>,
    returned: Result<Zval, String>,
    ty: &ComponentFunc,
    results: &mut [Val],
) -> (wasmtime::Result<()>, Vec<Zval>) {
    let store = store::of(ctx);
    let mut leftovers = Vec::new();
    let lent = host_resource::mark(ctx);
    let moves = resource::moves_mark(ctx);
    let outcome = match (&returned, ty.results().next()) {
        (Ok(_), None) => Ok(()),
        (Ok(value), Some(Type::Result(result))) => {
            let val = if downcast::<ResultValue>(value).is_some() {
                to_val(ctx, value, &Type::Result(result))
            } else {
                ok_payload(ctx, value, result.ok()).map(|payload| Val::Result(Ok(payload)))
            };
            val.map(|val| results[0] = val).map_err(Into::into)
        }
        (Ok(value), Some(result_ty)) => to_val(ctx, value, &result_ty)
            .map(|val| results[0] = val)
            .map_err(Into::into),
        // A ComponentError returns the err of a result to the component.
        (Err(_), Some(Type::Result(result))) if pending_component_error() => {
            let error = ExecutorGlobals::take_exception().expect("a pending ComponentError");
            let payload: Zval = error
                .get_property::<&Zval>("payload")
                .map_or_else(|_| Zval::null(), Zval::shallow_clone);
            let outcome = ok_payload(ctx, &payload, result.err())
                .map(|payload| results[0] = Val::Result(Err(payload)))
                .map_err(Into::into);
            leftovers.push(payload);
            if let Ok(error) = error.into_zval(false) {
                leftovers.push(error);
            }
            outcome
        }
        // Any other PHP exception stays pending in the engine while the
        // component unwinds, and the PHP entry point that started the call
        // rethrows it.
        (Err(err), _) => Err(wasmtime::Error::msg(format!("PHP callback failed: {err}"))),
    };
    // The component receives the converted result when this returns Ok.
    resource::finish_moves(ctx, moves, outcome.is_ok());
    let mut released = Vec::new();
    host_resource::reclaim(ctx, lent, outcome.is_ok(), &mut released);
    for object in released {
        store.put_garbage(object);
    }
    if let Ok(value) = returned {
        leftovers.push(value);
    }
    (outcome, leftovers)
}

/// A component import call in an async store. It lifts the parameters, asks
/// `suspend::drive` to run the PHP call on the PHP stack, and settles the
/// result when polled again.
struct ComponentHostCall<'a> {
    ctx: StoreContextMut<'a, HostState>,
    ty: ComponentFunc,
    params: &'a [Val],
    results: &'a mut [Val],
    what: Target,
    /// The request this call made, once it made it.
    id: Option<u64>,
}

impl std::future::Future for ComponentHostCall<'_> {
    type Output = wasmtime::Result<()>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        use std::task::Poll;

        let this = self.get_mut();
        let store = store::of(&this.ctx);
        let Some(id) = this.id else {
            let callable = callable_of(&this.ctx, &this.what);
            let args = match lift(&mut this.ctx, &store, &this.ty, this.params) {
                Ok(args) => args,
                Err(err) => return Poll::Ready(Err(err)),
            };
            let id = store.next_request_id();
            store.put_request(Request {
                id,
                // The future is pinned inside wasmtime, so this address holds until it is dropped.
                access: Active::Component(
                    (&mut this.ctx as *mut StoreContextMut<'_, HostState>).cast(),
                ),
                suspending: matches!(this.what, Target::Callable(_, true)),
                callee: Callee::Component(this.what.clone(), callable),
                args,
            });
            store.wait_for(id, cx.waker());
            this.id = Some(id);
            return Poll::Pending;
        };
        // wasmtime's concurrent loop may poll before the response is there.
        let Some(returned) = store.take_response(id) else {
            store.wait_for(id, cx.waker());
            return Poll::Pending;
        };
        let (outcome, leftovers) = settle(&mut this.ctx, returned, &this.ty, this.results);
        for value in leftovers {
            store.put_garbage(value);
        }
        Poll::Ready(outcome)
    }
}

/// A call of an `async func` import. wasmtime runs it on its concurrent
/// loop and gives store access only through `accessor` while it polls, so
/// the values are converted inside each poll, and the PHP callback runs
/// with no store access at all.
struct ConcurrentHostCall<'a> {
    accessor: &'a Accessor<HostState>,
    ty: ComponentFunc,
    params: &'a [Val],
    results: &'a mut [Val],
    what: Target,
    id: Option<u64>,
}

impl std::future::Future for ConcurrentHostCall<'_> {
    type Output = wasmtime::Result<()>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        use std::task::Poll;

        let this = self.get_mut();
        let accessor = this.accessor;
        accessor.with(|mut access| {
            let mut ctx = access.as_context_mut();
            let store = store::of(&ctx);
            let Some(id) = this.id else {
                let callable = callable_of(&ctx, &this.what);
                let args = match lift(&mut ctx, &store, &this.ty, this.params) {
                    Ok(args) => args,
                    Err(err) => return Poll::Ready(Err(err)),
                };
                let id = store.next_request_id();
                store.put_request(Request {
                    id,
                    access: Active::Unavailable,
                    suspending: matches!(this.what, Target::Callable(_, true)),
                    callee: Callee::Component(this.what.clone(), callable),
                    args,
                });
                store.wait_for(id, cx.waker());
                this.id = Some(id);
                return Poll::Pending;
            };
            let Some(returned) = store.take_response(id) else {
                store.wait_for(id, cx.waker());
                return Poll::Pending;
            };
            let (outcome, leftovers) = settle(&mut ctx, returned, &this.ty, this.results);
            for value in leftovers {
                store.put_garbage(value);
            }
            Poll::Ready(outcome)
        })
    }
}

/// Whether any function the component imports or exports, at the world
/// level or inside an interface, is an `async func`.
pub fn uses_async_abi(component: &Component) -> bool {
    let ty = component.inner.component_type();
    let mut items: Vec<ComponentItem> = ty
        .imports(engine())
        .chain(ty.exports(engine()))
        .map(|(_, item)| item.ty)
        .collect();
    while let Some(item) = items.pop() {
        match item {
            ComponentItem::ComponentFunc(func) if func.async_() => return true,
            ComponentItem::ComponentInstance(instance) => {
                items.extend(instance.exports(engine()).map(|(_, item)| item.ty));
            }
            _ => {}
        }
    }
    false
}

/// Whether any import is a `Wasm\Suspending`, at the world level or inside
/// an imported interface. Checked before linking, since every PHP import of
/// such an instance is async.
pub fn has_suspending(component: &Component, imports: Option<&ZendHashTable>) -> bool {
    let Some(imports) = imports else {
        return false;
    };
    component
        .inner
        .component_type()
        .imports(engine())
        .any(|(name, _)| {
            let unversioned = name.split('@').next().unwrap_or(name);
            let Some(value) = imports.get(name).or_else(|| imports.get(unversioned)) else {
                return false;
            };
            downcast::<Suspending>(value).is_some()
                || value.array().is_some_and(|functions| {
                    functions
                        .values()
                        .any(|value| downcast::<Suspending>(value).is_some())
                })
        })
}

pub fn call_target(
    what: &Target,
    callable: &Zval,
    args: &[Zval],
) -> ext_php_rs::error::Result<Zval> {
    match what {
        Target::Callable(..) => ZendCallable::new(callable)?.try_call(dyn_args(args)),
        Target::Static(name) => ZendCallable::try_from_name(name)?.try_call(dyn_args(args)),
        Target::Constructor(class) => {
            let class =
                ClassEntry::try_find(class).ok_or(ext_php_rs::error::Error::InvalidScope)?;
            let object = ZendObject::new(class);
            if !class.constructor.is_null() {
                object.try_call_method("__construct", dyn_args(args))?;
            }
            object.into_zval(false)
        }
        Target::ComponentConstructor(_) => {
            let class = downcast::<ResourceClass>(callable).ok_or(Error::InvalidScope)?;
            let args: Vec<&Zval> = args.iter().collect();
            class.construct(&args).map_err(thrown)
        }
        Target::ComponentStatic(_, name) => {
            let class = downcast::<ResourceClass>(callable).ok_or(Error::InvalidScope)?;
            let (_, function) = class
                .meta
                .statics
                .iter()
                .find(|(static_name, _)| static_name == name)
                .ok_or(Error::InvalidScope)?;
            let args: Vec<&Zval> = args.iter().collect();
            function.call(&args).map_err(thrown)
        }
        Target::Method(method) => {
            let (this, rest) = args.split_first().ok_or(Error::InvalidScope)?;
            if let Some(resource) = downcast::<Resource>(this) {
                let rest: Vec<&Zval> = rest.iter().collect();
                return resource.call(method.clone(), &rest).map_err(thrown);
            }
            let object = this.object().ok_or(Error::InvalidScope)?;
            object.try_call_method(&camel(method), dyn_args(rest))
        }
    }
}

/// Makes an error of another instance's call the pending PHP exception, as
/// a PHP callable's exception is, so the entry point rethrows it.
fn thrown(err: ext_php_rs::exception::PhpException) -> Error {
    if ExecutorGlobals::get().exception().is_none() {
        err.throw();
    }
    Error::Callable
}

fn dyn_args(args: &[Zval]) -> Vec<&dyn IntoZvalDyn> {
    args.iter().map(|arg| arg as &dyn IntoZvalDyn).collect()
}

fn pending_component_error() -> bool {
    ExecutorGlobals::get()
        .exception()
        .is_some_and(|exception| exception.instance_of(component_error()))
}

fn ok_payload(
    ctx: &mut StoreContextMut<'_, HostState>,
    value: &Zval,
    ty: Option<Type>,
) -> Result<Option<Box<Val>>, crate::value::ConvertError> {
    match ty {
        Some(ty) => Ok(Some(Box::new(to_val(ctx, value, &ty)?))),
        None => Ok(None),
    }
}
