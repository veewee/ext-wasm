use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

use ext_php_rs::types::Zval;
use wasmtime::{AsContextMut, Caller, RootScope, Store, StoreContextMut};

use crate::engine::engine;

#[derive(Default)]
pub struct HostState {
    pub values: Values,
    /// PHP objects of the tags PHP has seen, so a tag round trips by identity.
    pub tags: Vec<(wasmtime::Tag, Zval)>,
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

/// The wasmtime store shared by every wasm object on this PHP thread.
///
/// JS lets any Memory, Global, Table or function be combined with any instance,
/// while wasmtime requires them to live in one store. Every PHP wrapper holds an
/// `Rc` to this handle, so the store lives exactly as long as some wasm object
/// does.
pub struct StoreHandle {
    store: RefCell<Store<HostState>>,
    /// The caller of the host function that is currently running PHP code.
    ///
    /// While wasm runs, the outer call holds the `RefCell` borrow, so a PHP
    /// callback that touches any wasm object (calling another export, reading
    /// memory) must go through the caller wasmtime handed to the host function.
    active: Cell<*mut Caller<'static, HostState>>,
}

thread_local! {
    static CURRENT: RefCell<Weak<StoreHandle>> = const { RefCell::new(Weak::new()) };
}

pub type SharedStore = Rc<StoreHandle>;

pub fn current() -> SharedStore {
    CURRENT.with(|current| {
        if let Some(handle) = current.borrow().upgrade() {
            return handle;
        }
        let handle = Rc::new(StoreHandle {
            store: RefCell::new(Store::new(engine(), HostState::default())),
            active: Cell::new(std::ptr::null_mut()),
        });
        *current.borrow_mut() = Rc::downgrade(&handle);
        handle
    })
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
