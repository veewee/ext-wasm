//! WIT resources a component imports, implemented by PHP classes: the
//! component's handles point into a table of PHP objects.

use std::cell::RefCell;

use ext_php_rs::types::Zval;
use ext_php_rs::zend::ClassEntry;
use wasmtime::StoreContextMut;
use wasmtime::component::{ResourceAny, ResourceDynamic, ResourceType};

use crate::store::HostState;
use crate::value::{ConvertError, debug_type};

/// The PHP classes and objects behind the host resources of one store.
#[derive(Default)]
pub struct HostResources {
    /// The PHP class of each imported resource type, by its number.
    classes: Vec<String>,
    objects: Vec<Option<Zval>>,
    vacant: Vec<u32>,
}

impl HostResources {
    /// Registers the PHP class of an imported resource type and returns the
    /// type wasmtime tells it apart by.
    pub fn register(&mut self, class: String) -> ResourceType {
        self.classes.push(class);
        ResourceType::host_dynamic((self.classes.len() - 1) as u32)
    }

    /// The number and PHP class of `ty`, if PHP implements it.
    pub fn class_of(&self, ty: &ResourceType) -> Option<(u32, &str)> {
        (0..self.classes.len() as u32)
            .find(|n| ResourceType::host_dynamic(*n) == *ty)
            .map(|n| (n, self.classes[n as usize].as_str()))
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

thread_local! {
    /// PHP objects handed to the component while a call's values are
    /// converted, with whether each was given as its own. A call takes back
    /// its entries after it ends: the lent ones always, the owned ones when
    /// the component never received them.
    static HANDED: RefCell<Vec<(ResourceAny, bool)>> = const { RefCell::new(Vec::new()) };
}

/// Where the entries of the call about to convert its values start, for a
/// nested call must not take back those of the call around it.
pub fn mark() -> usize {
    HANDED.with(|handed| handed.borrow().len())
}

/// Hands a PHP object of the class behind `ty` to the component: as its own
/// for an `own` parameter, lent for a `borrow`.
pub fn lower(
    ctx: &mut StoreContextMut<'_, HostState>,
    value: &Zval,
    ty: &ResourceType,
    owned: bool,
) -> Result<Option<ResourceAny>, ConvertError> {
    let Some((n, class)) = ctx.data().host_resources.class_of(ty) else {
        return Ok(None);
    };
    let is_instance = ClassEntry::try_find(class).is_some_and(|class| {
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
    let rep = ctx.data_mut().host_resources.insert(value.shallow_clone());
    let handle = ResourceDynamic::new_own(rep, n)
        .try_into_resource_any(&mut *ctx)
        .map_err(|err| ConvertError::Runtime(format!("{err:#}")))?;
    HANDED.with(|handed| handed.borrow_mut().push((handle, owned)));
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
    let handed = HANDED.with(|handed| {
        let mut handed = handed.borrow_mut();
        let mark = mark.min(handed.len());
        handed.split_off(mark)
    });
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
    if ctx.data().host_resources.class_of(&handle.ty()).is_none() {
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
