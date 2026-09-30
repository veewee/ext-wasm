use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use ext_php_rs::types::Zval;
use wasmtime::{AsContextMut, Caller, Store, StoreContextMut};

use crate::engine::engine;

#[derive(Default)]
pub struct HostState {
    /// PHP values referenced from wasm (callables behind host functions).
    /// wasmtime requires host closures to be Send + Sync and Zval is neither,
    /// so closures capture a key into this list instead of the value.
    pub values: Values,
}

#[derive(Default)]
pub struct Values(Vec<Zval>);

impl Values {
    pub fn insert(&mut self, value: Zval) -> usize {
        self.0.push(value);
        self.0.len() - 1
    }

    pub fn get(&self, key: usize) -> &Zval {
        &self.0[key]
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

impl StoreHandle {
    pub fn with<R>(&self, f: impl FnOnce(StoreContextMut<'_, HostState>) -> R) -> R {
        let active = self.active.get();
        if active.is_null() {
            f(self.store.borrow_mut().as_context_mut())
        } else {
            // SAFETY: `active` is only non-null inside `enter_host`, which keeps
            // the caller alive for the duration and restores the previous value
            // before returning. No other reference to the store exists meanwhile:
            // the outer borrow is parked inside wasmtime's call.
            f(unsafe { &mut *active }.as_context_mut())
        }
    }

    /// Runs PHP code from inside a host function, routing store access through `caller`.
    pub fn enter_host<R>(&self, caller: &mut Caller<'_, HostState>, f: impl FnOnce() -> R) -> R {
        let previous = self.active.replace((caller as *mut Caller<'_, HostState>).cast());
        let _restore = Restore(&self.active, previous);
        f()
    }
}

struct Restore<'a>(&'a Cell<*mut Caller<'static, HostState>>, *mut Caller<'static, HostState>);

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        self.0.set(self.1);
    }
}
