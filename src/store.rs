use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

use ext_php_rs::exception::{PhpException, PhpResult};
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use wasmtime::{AsContext, AsContextMut, Caller, RootScope, Store, StoreContextMut};

use crate::engine::engine;
use crate::error::link_error;

#[derive(Default)]
pub struct HostState {
    pub values: Values,
    /// The PHP objects of tags PHP has seen, so a tag round trips by identity.
    pub tags: Vec<KnownTag>,
    /// The handle that owns this store. Weak, because the handle owns the store.
    handle: Weak<StoreHandle>,
}

/// A tag's PHP object, held without a reference.
///
/// The object owns the store, so a counted reference here would be a cycle
/// that PHP's garbage collector cannot see. `alive` is cleared when the object
/// is freed.
pub struct KnownTag {
    pub tag: wasmtime::Tag,
    pub object: *mut ext_php_rs::types::ZendObject,
    pub alive: Rc<Cell<bool>>,
}

/// PHP values referenced from wasm: callables behind host functions and
/// externref payloads.
///
/// wasmtime requires host closures and externref payloads to be Send + Sync and
/// Zval is neither, so wasm only ever holds a key into this list.
#[derive(Default)]
pub struct Values {
    slots: Vec<Option<Zval>>,
    /// Keys whose externref was collected by wasmtime's GC. The payload's Drop
    /// runs inside the GC, where the store data is out of reach.
    freed: Arc<Mutex<Vec<usize>>>,
    /// Slots taken from `freed` and ready for reuse.
    vacant: Vec<usize>,
    /// Zvals released from collected slots, dropped outside the store borrow
    /// because a PHP destructor may touch wasm objects again.
    released: Vec<Zval>,
    gc_threshold: usize,
}

/// The externref payload: a key that releases its slot when wasm drops the reference.
pub struct ValueKey {
    key: usize,
    freed: Arc<Mutex<Vec<usize>>>,
}

impl Drop for ValueKey {
    fn drop(&mut self) {
        if let Ok(mut freed) = self.freed.lock() {
            freed.push(self.key);
        }
    }
}

impl ValueKey {
    pub fn key(&self) -> usize {
        self.key
    }
}

impl Values {
    /// Stores a value for the lifetime of the store (host function callables).
    pub fn insert_permanent(&mut self, value: Zval) -> usize {
        self.insert(value)
    }

    /// Stores a value until wasm drops the returned key.
    pub fn insert_ref(&mut self, value: Zval) -> ValueKey {
        ValueKey {
            key: self.insert(value),
            freed: self.freed.clone(),
        }
    }

    pub fn get(&self, key: usize) -> &Zval {
        self.slots[key]
            .as_ref()
            .expect("wasm only holds keys of live values")
    }

    fn insert(&mut self, value: Zval) -> usize {
        self.reclaim();
        if let Some(key) = self.vacant.pop() {
            self.slots[key] = Some(value);
            key
        } else {
            self.slots.push(Some(value));
            self.slots.len() - 1
        }
    }

    fn reclaim(&mut self) {
        let freed = self
            .freed
            .lock()
            .map(|mut freed| std::mem::take(&mut *freed))
            .unwrap_or_default();
        for key in freed {
            if let Some(value) = self.slots[key].take() {
                self.released.push(value);
            }
            self.vacant.push(key);
        }
    }

    fn live(&self) -> usize {
        self.slots.len() - self.vacant.len()
    }
}

/// A wasmtime store and the PHP objects that share it.
///
/// wasmtime frees instances only together with their store, so every
/// instance gets its own store unless it is combined with others: see
/// `choose` and `standalone`. Every
/// PHP wrapper holds an `Rc` to this handle, so the store lives exactly as long
/// as some wasm object in it does.
pub struct StoreHandle {
    store: RefCell<Store<HostState>>,
    /// The caller of the host function that is currently running PHP code.
    ///
    /// While wasm runs, the outer call holds the `RefCell` borrow, so a PHP
    /// callback that touches any wasm object (calling another export, reading
    /// memory) must go through the caller wasmtime handed to the host function.
    active: Cell<*mut Caller<'static, HostState>>,
}

pub type SharedStore = Rc<StoreHandle>;

thread_local! {
    static STANDALONE: RefCell<Weak<StoreHandle>> = const { RefCell::new(Weak::new()) };
}

/// The store that standalone objects share when they are created without one.
///
/// JS imports a Memory and a Global created on their own into one instance,
/// which is only possible when both live in the same store. Instances get a
/// store of their own instead, so dropping one frees its memory.
pub fn standalone() -> SharedStore {
    STANDALONE.with(|standalone| {
        if let Some(handle) = standalone.borrow().upgrade() {
            return handle;
        }
        let handle = new();
        *standalone.borrow_mut() = Rc::downgrade(&handle);
        handle
    })
}

/// Creates a store of its own.
pub fn new() -> SharedStore {
    Rc::new_cyclic(|handle| StoreHandle {
        store: RefCell::new(Store::new(
            engine(),
            HostState {
                handle: handle.clone(),
                ..HostState::default()
            },
        )),
        active: Cell::new(std::ptr::null_mut()),
    })
}

/// The handle of the store `ctx` belongs to.
pub fn of(ctx: &impl AsContext<Data = HostState>) -> SharedStore {
    ctx.as_context()
        .data()
        .handle
        .upgrade()
        // Wasm code and conversions only run while some wrapper holds the handle.
        .expect("a store in use has a live handle")
}

/// Groups wasm objects so they can be combined.
///
/// An object created without a store joins the store of the wasm objects it
/// is built from. Otherwise an instance gets a store of its own, and a
/// Memory, Table, GlobalVar or Tag joins the store all such standalone
/// objects share. wasmtime frees memory one whole store at a time, when no
/// object in it is left.
#[php_class]
#[php(name = "Wasm\\Store")]
#[php(flags = ClassFlags::Final)]
pub struct StoreObject {
    pub handle: SharedStore,
}

#[php_impl]
impl StoreObject {
    pub fn __construct() -> Self {
        Self { handle: new() }
    }
}

/// Picks the store of a new object: the explicit one, otherwise the store of
/// the wasm objects it is built from, otherwise `fallback`.
pub fn choose(
    explicit: Option<&StoreObject>,
    from: impl IntoIterator<Item = (SharedStore, &'static str)>,
    fallback: fn() -> SharedStore,
) -> PhpResult<SharedStore> {
    let mut chosen = explicit.map(|store| store.handle.clone());
    for (store, kind) in from {
        match &chosen {
            None => chosen = Some(store),
            Some(existing) if Rc::ptr_eq(existing, &store) => {}
            Some(_) => return Err(mismatch(kind)),
        }
    }
    Ok(chosen.unwrap_or_else(fallback))
}

/// Whether `store` is the store `ctx` belongs to.
pub fn owns(ctx: &impl AsContext<Data = HostState>, store: &SharedStore) -> bool {
    std::ptr::eq(ctx.as_context().data().handle.as_ptr(), Rc::as_ptr(store))
}

pub fn mismatch_message(kind: &str) -> String {
    format!("{kind} belongs to a different store; create both with store: $store")
}

/// wasmtime aborts when objects of two stores meet, so this is checked first.
pub fn mismatch(kind: &str) -> PhpException {
    link_error(mismatch_message(kind))
}

const MIN_GC_THRESHOLD: usize = 1024;

impl StoreHandle {
    /// Runs `f` with the store. Every GC root created inside `f` is released
    /// when it returns, so PHP values held by wasm only stay alive as long as
    /// wasm itself references them.
    pub fn with<R>(&self, f: impl FnOnce(StoreContextMut<'_, HostState>) -> R) -> R {
        let active = self.active.get();
        if !active.is_null() {
            // SAFETY: `active` is only non-null inside `enter_host`, which keeps
            // the caller alive for the duration and restores the previous value
            // before returning. No other reference to the store exists meanwhile:
            // the outer borrow is parked inside wasmtime's call.
            let mut scope = RootScope::new(unsafe { &mut *active });
            return f(scope.as_context_mut());
        }

        let result = {
            let mut store = self.store.borrow_mut();
            let mut scope = RootScope::new(&mut *store);
            f(scope.as_context_mut())
        };
        drop(self.collect());
        result
    }

    /// Runs PHP code from inside a host function, routing store access through `caller`.
    pub fn enter_host<R>(&self, caller: &mut Caller<'_, HostState>, f: impl FnOnce() -> R) -> R {
        let previous = self
            .active
            .replace((caller as *mut Caller<'_, HostState>).cast());
        let _restore = Restore(&self.active, previous);
        f()
    }

    /// Collects unreferenced externrefs once enough PHP values piled up, and
    /// hands back the released values so the caller drops them without a borrow.
    fn collect(&self) -> Vec<Zval> {
        let mut store = self.store.borrow_mut();
        if store.data().values.live() >= store.data().values.gc_threshold.max(MIN_GC_THRESHOLD) {
            // A failing GC only delays reclaiming memory, it is not an error for the caller.
            let _ = store.gc(None);
            let values = &mut store.data_mut().values;
            values.reclaim();
            values.gc_threshold = values.live() * 2;
        }
        std::mem::take(&mut store.data_mut().values.released)
    }
}

struct Restore<'a>(
    &'a Cell<*mut Caller<'static, HostState>>,
    *mut Caller<'static, HostState>,
);

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        self.0.set(self.1);
    }
}
