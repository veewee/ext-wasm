//! WIT resources a component imports, implemented by PHP classes: the
//! component's handles point into a table of PHP objects.

use ext_php_rs::types::Zval;
use ext_php_rs::zend::ClassEntry;
use wasmtime::StoreContextMut;
use wasmtime::component::{ResourceAny, ResourceDynamic, ResourceType};

use std::rc::Rc;

use crate::component::resource::{Resource, ResourceClass};
use crate::store::HostState;
use crate::value::{ConvertError, debug_type, downcast};

/// What implements a resource a component imports.
#[derive(Clone)]
pub enum HostImpl {
    /// A PHP class, whose objects the component gets handles to.
    Class(String),
    /// The resource another component instance exports, by the key of its
    /// `Wasm\Component\ResourceClass` in the store's values. The handles
    /// are that instance's `Wasm\Component\Resource` objects.
    Component(usize),
}

/// The PHP classes and objects behind the host resources of one store.
#[derive(Default)]
pub struct HostResources {
    /// The implementation of each imported resource type, by its number.
    classes: Vec<HostImpl>,
    objects: Vec<Option<Zval>>,
    vacant: Vec<u32>,
    /// PHP objects handed to the component while a call's values are
    /// converted, with whether each was given as its own. A call takes back
    /// its entries after it ends: the lent ones always, the owned ones when
    /// the component never received them. Kept per store, not per thread,
    /// because a call suspended in one Fiber must not see another Fiber's.
    handed: Vec<(ResourceAny, bool)>,
}

impl HostResources {
    /// Registers the PHP class of an imported resource type and returns the
    /// type wasmtime tells it apart by.
    pub fn register(&mut self, implementation: HostImpl) -> ResourceType {
        self.classes.push(implementation);
        ResourceType::host_dynamic((self.classes.len() - 1) as u32)
    }

    /// The number and implementation of `ty`, if PHP implements it.
    pub fn impl_of(&self, ty: &ResourceType) -> Option<(u32, HostImpl)> {
        (0..self.classes.len() as u32)
            .find(|n| ResourceType::host_dynamic(*n) == *ty)
            .map(|n| (n, self.classes[n as usize].clone()))
    }

    fn insert(&mut self, object: Zval) -> u32 {
        match self.vacant.pop() {
            Some(rep) => {
                self.objects[rep as usize] = Some(object);
                rep
            }
            None => {
                self.objects.push(Some(object));
                (self.objects.len() - 1) as u32
            }
        }
    }

    fn get(&self, rep: u32) -> Option<&Zval> {
        self.objects.get(rep as usize).and_then(Option::as_ref)
    }

    /// Removes the object behind `rep`, when the component dropped it or gave it back.
    pub fn take(&mut self, rep: u32) -> Option<Zval> {
        let object = self.objects.get_mut(rep as usize)?.take();
        if object.is_some() {
            self.vacant.push(rep);
        }
        object
    }
}

/// Where the entries of the call about to convert its values start, for a
/// nested call must not take back those of the call around it.
pub fn mark(ctx: &StoreContextMut<'_, HostState>) -> usize {
    ctx.data().host_resources.handed.len()
}

/// Hands a PHP object of the class behind `ty` to the component: as its own
/// for an `own` parameter, lent for a `borrow`.
pub fn lower(
    ctx: &mut StoreContextMut<'_, HostState>,
    value: &Zval,
    ty: &ResourceType,
    owned: bool,
) -> Result<Option<ResourceAny>, ConvertError> {
    let Some((n, implementation)) = ctx.data().host_resources.impl_of(ty) else {
        return Ok(None);
    };
    match implementation {
        HostImpl::Class(class) => {
            let is_instance = ClassEntry::try_find(&class).is_some_and(|class| {
                value
                    .object()
                    .is_some_and(|object| object.instance_of(class))
            });
            if !is_instance {
                return Err(ConvertError::Type(format!(
                    "expected {class}, got {}",
                    debug_type(value)
                )));
            }
        }
        HostImpl::Component(key) => {
            let class = downcast::<ResourceClass>(ctx.data().values.get(key))
                .map(|class| (class.store.clone(), class.meta.clone()));
            let Some((store, meta)) = class else {
                return Err(ConvertError::Runtime("the resource class is gone".into()));
            };
            let Some(resource) =
                downcast::<Resource>(value).filter(|resource| Rc::ptr_eq(resource.store(), &store))
            else {
                return Err(ConvertError::Type(format!(
                    "expected a {} resource of the instance exporting it, got {}",
                    meta.name,
                    debug_type(value)
                )));
            };
            // A moved or dropped handle would sit in the table as a dead object.
            let handle = resource
                .handle()
                .map_err(|err| ConvertError::Type(err.into()))?;
            if handle.ty() != meta.ty {
                return Err(ConvertError::Type(format!(
                    "expected a {} resource, got another resource of that instance",
                    meta.name
                )));
            }
        }
    }
    let rep = ctx.data_mut().host_resources.insert(value.shallow_clone());
    let handle = ResourceDynamic::new_own(rep, n)
        .try_into_resource_any(&mut *ctx)
        .map_err(|err| ConvertError::Runtime(format!("{err:#}")))?;
    ctx.data_mut().host_resources.handed.push((handle, owned));
    Ok(Some(handle))
}

/// Takes back the objects handed to the component since `mark`: the lent
/// ones, and the owned ones too when `delivered` is false because the
/// component never received them.
pub fn reclaim(
    ctx: &mut StoreContextMut<'_, HostState>,
    mark: usize,
    delivered: bool,
    released: &mut Vec<Zval>,
) {
    let handed = {
        let handed = &mut ctx.data_mut().host_resources.handed;
        let mark = mark.min(handed.len());
        handed.split_off(mark)
    };
    for (handle, owned) in handed {
        if owned && delivered {
            continue;
        }
        // An owned handle the component already took fails here, and stays its.
        if let Ok(resource) = handle.try_into_resource_dynamic(&mut *ctx)
            && let Some(object) = ctx.data_mut().host_resources.take(resource.rep())
        {
            released.push(object);
        }
    }
}

/// The PHP object behind a host resource the component passes to PHP: taken
/// out of the table for an `own`, shared for a `borrow`. `None` when the
/// handle is not a host resource.
pub fn lift(
    ctx: &mut StoreContextMut<'_, HostState>,
    handle: &ResourceAny,
) -> Result<Option<Zval>, ConvertError> {
    if ctx.data().host_resources.impl_of(&handle.ty()).is_none() {
        return Ok(None);
    }
    let owned = handle.owned();
    // For a borrow this also ends the borrow, as wasmtime requires before the host call returns.
    let resource = handle
        .try_into_resource_dynamic(&mut *ctx)
        .map_err(|err| ConvertError::Runtime(format!("{err:#}")))?;
    let objects = &mut ctx.data_mut().host_resources;
    let object = if owned {
        objects.take(resource.rep())
    } else {
        objects.get(resource.rep()).map(Zval::shallow_clone)
    };
    object.map(Some).ok_or_else(|| {
        ConvertError::Runtime("the component passed a host resource that no longer exists".into())
    })
}
