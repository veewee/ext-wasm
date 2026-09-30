//! WIT resources a component exports: a class with a constructor and static
//! functions, and handles with methods that PHP drops like any object.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime::component::{ResourceAny, ResourceType};

use crate::component::func::Func;
use crate::component::value::camel;
use crate::error::error;
use crate::store::SharedStore;

/// What a resource type offers, shared by its class and its handles.
pub struct ResourceMeta {
    pub name: String,
    pub ty: ResourceType,
    pub constructor: Option<Func>,
    /// By camelCase name.
    pub statics: Vec<(String, Func)>,
    /// By WIT name, as after `[method]name.`.
    pub methods: Vec<(String, Func)>,
}

impl ResourceMeta {
    /// Groups the `[constructor]name`, `[static]name.x` and `[method]name.x`
    /// functions of an interface under the resource `name`.
    pub fn new(
        store: &SharedStore,
        name: &str,
        ty: ResourceType,
        functions: &[(String, wasmtime::component::Func)],
    ) -> Self {
        let func = |inner: &wasmtime::component::Func| Func {
            store: store.clone(),
            inner: *inner,
        };
        let constructor_name = format!("[constructor]{name}");
        let static_prefix = format!("[static]{name}.");
        let method_prefix = format!("[method]{name}.");
        let mut meta = Self {
            name: name.to_string(),
            ty,
            constructor: None,
            statics: Vec::new(),
            methods: Vec::new(),
        };
        for (export, inner) in functions {
            if *export == constructor_name {
                meta.constructor = Some(func(inner));
            } else if let Some(function) = export.strip_prefix(&static_prefix) {
                meta.statics.push((camel(function), func(inner)));
            } else if let Some(method) = export.strip_prefix(&method_prefix) {
                meta.methods.push((method.to_string(), func(inner)));
            }
        }
        meta
    }
}

/// Whether an export name belongs to a resource rather than the interface.
pub fn is_resource_function(name: &str) -> bool {
    name.starts_with("[constructor]")
        || name.starts_with("[static]")
        || name.starts_with("[method]")
}

/// A resource type a component exports: `new(...)` constructs it, and its
/// static functions are camelCase methods.
#[php_class]
#[php(name = "Wasm\\Component\\ResourceClass")]
#[php(flags = ClassFlags::Final)]
pub struct ResourceClass {
    pub store: SharedStore,
    pub meta: Rc<ResourceMeta>,
}

#[php_impl]
impl ResourceClass {
    /// Calls the resource's constructor.
    #[php(name = "new")]
    pub fn construct(&self, args: &[&Zval]) -> PhpResult<Zval> {
        let constructor = self.meta.constructor.as_ref().ok_or_else(|| {
            error(format!(
                "resource \"{}\" has no constructor",
                self.meta.name
            ))
        })?;
        constructor.call(args)
    }

    pub fn __call(&self, name: String, arguments: &ZendHashTable) -> PhpResult<Zval> {
        let (_, function) = self
            .meta
            .statics
            .iter()
            .find(|(static_name, _)| *static_name == name)
            .ok_or_else(|| {
                error(format!(
                    "resource \"{}\" has no static function named \"{name}\"",
                    self.meta.name
                ))
            })?;
        let args: Vec<&Zval> = arguments.values().collect();
        function.call(&args)
    }

    pub fn __clone(&self) -> PhpResult<()> {
        Err(error("a resource class cannot be cloned"))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Live,
    Dropped,
    Moved,
}

/// A handle to a resource owned by a component instance. Methods call the
/// component; `drop()` releases the handle, as does the destructor.
#[php_class]
#[php(name = "Wasm\\Component\\Resource")]
#[php(flags = ClassFlags::Final)]
pub struct Resource {
    store: SharedStore,
    meta: Option<Rc<ResourceMeta>>,
    handle: Cell<Option<ResourceAny>>,
    state: Cell<State>,
}

#[php_impl]
impl Resource {
    pub fn __call(&self, name: String, arguments: &ZendHashTable) -> PhpResult<Zval> {
        let method = self
            .meta
            .as_ref()
            .and_then(|meta| {
                meta.methods
                    .iter()
                    .find(|(method, _)| camel(method) == name)
            })
            .map(|(method, _)| method.clone())
            .ok_or_else(|| error(format!("resource has no method named \"{name}\"")))?;
        let args: Vec<&Zval> = arguments.values().collect();
        self.invoke(&method, &args)
    }

    /// Calls the method `name` by its WIT name, for a method called `drop`.
    pub fn call(&self, name: String, args: &[&Zval]) -> PhpResult<Zval> {
        self.invoke(&name, args)
    }

    /// Releases the handle; the component runs its destructor for the resource.
    pub fn drop(&self) -> PhpResult<()> {
        if self.state.get() == State::Live {
            self.state.set(State::Dropped);
            if let Some(handle) = self.handle.take() {
                self.store.drop_resource(handle);
            }
        }
        Ok(())
    }

    pub fn __clone(&self) -> PhpResult<()> {
        Err(error("a resource handle cannot be cloned"))
    }
}

impl Resource {
    pub fn new(store: SharedStore, meta: Option<Rc<ResourceMeta>>, handle: ResourceAny) -> Self {
        Self {
            store,
            meta,
            handle: Cell::new(Some(handle)),
            state: Cell::new(State::Live),
        }
    }

    pub fn store(&self) -> &SharedStore {
        &self.store
    }

    /// The handle, for passing the resource to the component.
    pub fn handle(&self) -> Result<ResourceAny, &'static str> {
        match (self.state.get(), self.handle.get()) {
            (State::Live, Some(handle)) => Ok(handle),
            (State::Moved, _) => Err("the resource was moved into the component"),
            _ => Err("the resource was dropped"),
        }
    }

    /// Gives the handle to the component for an `own` parameter.
    pub fn mark_moved(&self) {
        self.state.set(State::Moved);
        self.handle.set(None);
    }

    fn invoke(&self, method: &str, args: &[&Zval]) -> PhpResult<Zval> {
        let handle = self.handle().map_err(error)?;
        let function = self
            .meta
            .as_ref()
            .and_then(|meta| meta.methods.iter().find(|(name, _)| name == method))
            .map(|(_, function)| function)
            .ok_or_else(|| error(format!("resource has no method named \"{method}\"")))?;
        function.call_with_self(Some(wasmtime::component::Val::Resource(handle)), args)
    }
}

thread_local! {
    /// Resources given for `own` parameters of the call being converted. They
    /// move into the component only once every argument converted.
    static MOVES: RefCell<Vec<*const Resource>> = const { RefCell::new(Vec::new()) };
}

/// The handle of `resource` for an `own` parameter, moved on `commit_moves(true)`.
pub fn take_for_own(resource: &Resource) -> Result<ResourceAny, String> {
    let handle = resource.handle().map_err(str::to_string)?;
    MOVES.with(|moves| {
        let mut moves = moves.borrow_mut();
        let resource: *const Resource = resource;
        if moves.contains(&resource) {
            return Err("the same resource cannot be given twice as an own value".to_string());
        }
        moves.push(resource);
        Ok(handle)
    })
}

/// Ends a conversion: its `own` resources move into the component if it succeeded.
pub fn commit_moves(succeeded: bool) {
    let moved = MOVES.with(|moves| std::mem::take(&mut *moves.borrow_mut()));
    if succeeded {
        for resource in moved {
            // SAFETY: the PHP objects are the arguments of the current call,
            // which hold them alive until the call returns.
            unsafe { (*resource).mark_moved() };
        }
    }
}

impl Drop for Resource {
    fn drop(&mut self) {
        if self.state.get() == State::Live
            && let Some(handle) = self.handle.take()
        {
            self.store.drop_resource(handle);
        }
    }
}
