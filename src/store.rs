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
use crate::suspend;

#[derive(Default)]
pub struct HostState {
    pub values: Values,
    /// The PHP objects of tags PHP has seen, so a tag round trips by identity.
    pub tags: Vec<KnownTag>,
    /// The handle that owns this store. Weak, because the handle owns the store.
    handle: Weak<StoreHandle>,
    /// The WASI context of a store created by `Wasm\Wasi`.
    pub wasi: Option<wasmtime_wasi::p1::WasiP1Ctx>,
    /// The preview2 WASI context of a component instance given a `Wasm\Wasi`.
    pub wasi_p2: Option<WasiP2>,
    /// Outgoing wasi:http of a component instance given a `Wasm\\Wasi` with httpHosts.
    pub http: Option<crate::component::http::WasiHttp>,
    /// The PHP objects behind resources a component imports.
    pub host_resources: crate::component::host_resource::HostResources,
    /// Component resources given for `own` values of the calls being made,
    /// which move into the component once their call worked.
    pub moves: Vec<*const crate::component::resource::Resource>,
    /// Set once an instance with a `Wasm\Suspending` import joins this store.
    /// From then on every PHP callback is async and every call goes through
    /// `suspend::drive`, because wasmtime rejects sync calls in the store.
    pub is_async: bool,
    /// Whether a sync PHP host function exists, which rules out turning async.
    pub sync_callbacks: bool,
    /// The memory of this store, counted against wasm.memory_limit.
    pub memory: crate::limits::MemoryBudget,
}

// SAFETY: wasmtime-wasi and wasmtime's async functions require Send store
// data. A store is created, used and dropped on one PHP thread and never
// handed to another, so the Rc and raw pointers inside are never touched from
// two threads. The sync WASI functions run every host call on the calling
// thread, and `suspend::drive` polls every async call on that thread with a
// no-op waker. Preview2 file streams do hand reads and writes to tokio's
// blocking pool, but those tasks own their buffers and file handles, never
// the store data. Async WASI would need this revisited.
unsafe impl Send for HostState {}

pub struct WasiP2 {
    pub ctx: wasmtime_wasi::WasiCtx,
    pub table: wasmtime::component::ResourceTable,
}

impl wasmtime_wasi::WasiView for HostState {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        let p2 = self
            .wasi_p2
            .as_mut()
            .expect("preview2 WASI functions only exist in stores given a Wasm\\Wasi");
        wasmtime_wasi::WasiCtxView {
            ctx: &mut p2.ctx,
            table: &mut p2.table,
        }
    }
}

impl wasmtime_wasi_http::WasiHttpView for HostState {
    fn http(&mut self) -> wasmtime_wasi_http::WasiHttpCtxView<'_> {
        let (Some(p2), Some(http)) = (self.wasi_p2.as_mut(), self.http.as_mut()) else {
            panic!("wasi:http functions only exist in stores given a Wasm\\Wasi with httpHosts");
        };
        wasmtime_wasi_http::WasiHttpCtxView {
            hooks: &mut http.hooks,
            table: &mut p2.table,
            ctx: &mut http.ctx,
        }
    }
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

    /// Keeps a Zval to drop once the store borrow ends, for code that must
    /// not run PHP destructors itself.
    pub fn release(&mut self, value: Zval) {
        self.released.push(value);
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
    /// The store access of the host function that is currently running PHP code.
    ///
    /// While wasm runs, the outer call holds the `RefCell` borrow, so a PHP
    /// callback that touches any wasm object (calling another export, reading
    /// memory) must go through what wasmtime handed to the host function.
    active: Cell<Active>,
    /// Set while an async call waits outside wasm for its PHP callback. Its
    /// wasm frames are then off the activation list that wasmtime's GC walks,
    /// so nothing that can run the GC may touch the store: see `busy`.
    parked: Cell<bool>,
    /// PHP callbacks waiting calls ask for, in order. An async component
    /// can have several import calls in flight at once.
    requests: RefCell<std::collections::VecDeque<suspend::Request>>,
    /// What those callbacks returned, by request id.
    responses: RefCell<Vec<(u64, suspend::Response)>>,
    /// The wakers of host futures waiting for a response, by request id.
    /// wasmtime's concurrent loop only polls a host future again once its
    /// waker fired.
    wakers: RefCell<Vec<(u64, std::task::Waker)>>,
    next_request: Cell<u64>,
    /// Values a parked call's callback handed back, dropped where PHP code may
    /// run: before the next callback, or once the store borrow ends.
    garbage: RefCell<Vec<Zval>>,
    /// Component resource handles released while the store was in use, for
    /// example by a PHP destructor during a call. Dropped after the call.
    pending_drops: RefCell<Vec<wasmtime::component::ResourceAny>>,
    /// Component streams and futures PHP dropped unread while the store was in use.
    pending_closes: RefCell<Vec<Unread>>,
    /// Whether the store runs WASI, whose functions need the tokio runtime
    /// entered while wasm runs.
    uses_wasi: Cell<bool>,
    /// The resource types component instances in this store export, so a
    /// handle the component returns gets its methods.
    pub resource_types: RefCell<Vec<Rc<crate::component::resource::ResourceMeta>>>,
}

pub type SharedStore = Rc<StoreHandle>;

/// A component stream or future PHP dropped before reading it.
pub enum Unread {
    Stream(wasmtime::component::StreamAny),
    Future(wasmtime::component::FutureAny),
}

/// What a running host function received from wasmtime: a core function gets
/// a `Caller`, a component function a `StoreContextMut`.
#[derive(Clone, Copy)]
pub enum Active {
    None,
    Core(*mut Caller<'static, HostState>),
    Component(*mut StoreContextMut<'static, HostState>),
    /// An async component import, which wasmtime gives no store access
    /// outside its polls: PHP code it runs cannot use the store at all.
    Unavailable,
}

thread_local! {
    /// With the wasm.memory_limit it was created with.
    static STANDALONE: RefCell<(Weak<StoreHandle>, Option<u64>)> =
        const { RefCell::new((Weak::new(), None)) };
}

/// The store that standalone objects share when they are created without one.
///
/// JS imports a Memory and a Global created on their own into one instance,
/// which is only possible when both live in the same store. Instances get a
/// store of their own instead, so dropping one frees its memory.
pub fn standalone() -> SharedStore {
    let limit = crate::limits::memory_limit();
    STANDALONE.with(|standalone| {
        // A changed limit starts a new store, so a limit set around one
        // library call does not reach objects created before or after it.
        if let (shared, created_with) = &*standalone.borrow()
            && *created_with == limit
            && let Some(handle) = shared.upgrade()
        {
            return handle;
        }
        let handle = new();
        *standalone.borrow_mut() = (Rc::downgrade(&handle), limit);
        handle
    })
}

/// Stops handing out `store` to new standalone objects once an instance uses it.
///
/// A callback that captures an imported object keeps its store alive in a
/// cycle PHP cannot collect. Retiring the store limits that to the objects
/// created for this instance, instead of every standalone object after it.
pub fn retire_standalone(store: &SharedStore) {
    STANDALONE.with(|standalone| {
        let mut standalone = standalone.borrow_mut();
        if std::ptr::eq(standalone.0.as_ptr(), Rc::as_ptr(store)) {
            standalone.0 = Weak::new();
        }
    });
}

/// Creates a store of its own, with the limits currently set.
pub fn new() -> SharedStore {
    let memory_limit = crate::limits::memory_limit();
    Rc::new_cyclic(|handle| StoreHandle {
        store: RefCell::new({
            let mut store = Store::new(
                engine(),
                HostState {
                    handle: handle.clone(),
                    memory: crate::limits::MemoryBudget::new(memory_limit),
                    ..HostState::default()
                },
            );
            store.limiter(|state| &mut state.memory);
            store
        }),
        active: Cell::new(Active::None),
        parked: Cell::new(false),
        requests: RefCell::new(std::collections::VecDeque::new()),
        responses: RefCell::new(Vec::new()),
        wakers: RefCell::new(Vec::new()),
        next_request: Cell::new(0),
        garbage: RefCell::new(Vec::new()),
        pending_drops: RefCell::new(Vec::new()),
        pending_closes: RefCell::new(Vec::new()),
        uses_wasi: Cell::new(false),
        resource_types: RefCell::new(Vec::new()),
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
        match self.active.get() {
            Active::None => {}
            Active::Component(ctx) => {
                // SAFETY: only set inside `enter_component`, which keeps the
                // context alive for the duration and restores the previous
                // value before returning. The outer borrow waits inside
                // wasmtime's call meanwhile.
                let mut scope = RootScope::new(unsafe { &mut *ctx });
                return f(scope.as_context_mut());
            }
            // SAFETY: `Core` is only set inside `enter_host`, or while
            // `park` holds the caller of a parked async call, which stays at a
            // stable address inside wasmtime's pinned future. Both restore the
            // previous value when they end. No other reference to the store is
            // used meanwhile: the outer borrow waits inside wasmtime's call or
            // inside `suspend::drive`.
            //
            // wasmtime documents a suspended async call as keeping the store
            // (`StoreFiberYield::KeepStore`). Using it through the parked caller
            // relies on wasmtime 49 only swapping the executor, stack limit and
            // thread-local activations on suspend (runtime/fiber.rs), and on
            // `busy` keeping out everything that enters wasm or can run the GC.
            // Check this again whenever wasmtime is upgraded.
            Active::Core(caller) => {
                let mut scope = RootScope::new(unsafe { &mut *caller });
                return f(scope.as_context_mut());
            }
            // Every PHP entry point checks `is_parked` first and throws the
            // busy error, so this is only reached by a missing check.
            Active::Unavailable => panic!("{BUSY}"),
        }

        let result = {
            let mut store = self.store.borrow_mut();
            let wasi = store.data().wasi.is_some() || store.data().wasi_p2.is_some();
            self.uses_wasi.set(wasi);
            // A call into an async store can suspend its Fiber, and tokio
            // requires its enter guards to drop in reverse order, which
            // Fibers resumed out of order break. `suspend::drive` enters the
            // runtime for each poll there instead.
            let runtime = (wasi && !store.data().is_async).then(crate::engine::wasi_runtime);
            let _entered = runtime.as_ref().map(tokio::runtime::Handle::enter);
            let mut scope = RootScope::new(&mut *store);
            f(scope.as_context_mut())
        };
        self.drop_pending_resources();
        drop(self.collect());
        result
    }

    /// Releases a component resource handle, or queues it until the store is
    /// free when a call is running: dropping runs the component's destructor,
    /// which would enter the component again.
    pub fn drop_resource(&self, handle: wasmtime::component::ResourceAny) {
        let busy =
            !matches!(self.active.get(), Active::None) || self.store.try_borrow_mut().is_err();
        self.pending_drops.borrow_mut().push(handle);
        if !busy {
            self.drop_pending_resources();
        }
    }

    /// Closes a component stream or future PHP dropped unread, or queues it
    /// until the store is free, like `drop_resource`.
    pub fn close_unread(&self, unread: Unread) {
        let busy =
            !matches!(self.active.get(), Active::None) || self.store.try_borrow_mut().is_err();
        self.pending_closes.borrow_mut().push(unread);
        if !busy {
            self.drop_pending_resources();
        }
    }

    fn drop_pending_resources(&self) {
        loop {
            let closes = std::mem::take(&mut *self.pending_closes.borrow_mut());
            let pending = std::mem::take(&mut *self.pending_drops.borrow_mut());
            if pending.is_empty() && closes.is_empty() {
                return;
            }
            let mut store = self.store.borrow_mut();
            for unread in closes {
                // One of an instance that trapped is freed with the store.
                let _ = match unread {
                    Unread::Stream(mut stream) => stream.close(&mut *store),
                    Unread::Future(mut future) => future.close(&mut *store),
                };
            }
            let is_async = store.data().is_async;
            for handle in pending {
                // A handle of an instance that trapped cannot be dropped; the
                // store frees it together with the instance.
                if is_async {
                    // This runs from PHP destructors, even during garbage
                    // collection, where the component's destructor must not
                    // suspend the Fiber.
                    let _no_fiber_switch = crate::callback::FiberSwitchBlock::new();
                    let _ = crate::suspend::drive(self, handle.resource_drop_async(&mut *store));
                } else {
                    let _ = handle.resource_drop(&mut *store);
                }
            }
        }
    }

    /// Runs PHP code from inside a host function, routing store access through `caller`.
    pub fn enter_host<R>(&self, caller: &mut Caller<'_, HostState>, f: impl FnOnce() -> R) -> R {
        let previous = self
            .active
            .replace(Active::Core((caller as *mut Caller<'_, HostState>).cast()));
        let _restore = Restore(&self.active, previous);
        f()
    }

    /// Runs PHP code from inside a component host function, routing store
    /// access through `ctx`.
    pub fn enter_component<R>(
        &self,
        ctx: &mut StoreContextMut<'_, HostState>,
        f: impl FnOnce() -> R,
    ) -> R {
        let previous = self.active.replace(Active::Component(
            (ctx as *mut StoreContextMut<'_, HostState>).cast(),
        ));
        let _restore = Restore(&self.active, previous);
        f()
    }

    /// Routes store access through the caller of a parked async call until
    /// the guard drops.
    pub fn park(&self, access: Active) -> Parked<'_> {
        let previous = self.active.replace(access);
        let was_parked = self.parked.replace(true);
        Parked(self, previous, was_parked)
    }

    pub fn uses_wasi(&self) -> bool {
        self.uses_wasi.get()
    }

    pub fn is_parked(&self) -> bool {
        self.parked.get()
    }

    /// Turns the store async before any host function of an instance with
    /// Suspending imports is created.
    pub fn make_async(&self) -> PhpResult<()> {
        self.with(|mut ctx| {
            let state = ctx.data_mut();
            if state.sync_callbacks {
                return Err(link_error(
                    "Suspending imports need a store without synchronous callbacks",
                ));
            }
            state.is_async = true;
            Ok(())
        })
    }

    pub fn next_request_id(&self) -> u64 {
        let id = self.next_request.get();
        self.next_request.set(id.wrapping_add(1));
        id
    }

    pub fn put_request(&self, request: suspend::Request) {
        self.requests.borrow_mut().push_back(request);
    }

    pub fn take_request(&self) -> Option<suspend::Request> {
        self.requests.borrow_mut().pop_front()
    }

    /// Leaves the response to request `id` and wakes the future waiting for it.
    pub fn put_response(&self, id: u64, response: suspend::Response) {
        self.responses.borrow_mut().push((id, response));
        let waker = {
            let mut wakers = self.wakers.borrow_mut();
            let at = wakers.iter().position(|(waiting, _)| *waiting == id);
            at.map(|at| wakers.swap_remove(at).1)
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    pub fn take_response(&self, id: u64) -> Option<suspend::Response> {
        let mut responses = self.responses.borrow_mut();
        let at = responses.iter().position(|(answered, _)| *answered == id)?;
        Some(responses.swap_remove(at).1)
    }

    /// Remembers the waker of the future waiting for request `id`, replacing
    /// the one of an earlier poll.
    pub fn wait_for(&self, id: u64, waker: &std::task::Waker) {
        let mut wakers = self.wakers.borrow_mut();
        match wakers.iter_mut().find(|(waiting, _)| *waiting == id) {
            Some((_, stored)) => stored.clone_from(waker),
            None => wakers.push((id, waker.clone())),
        }
    }

    pub fn put_garbage(&self, value: Zval) {
        self.garbage.borrow_mut().push(value);
    }

    pub fn take_garbage(&self) -> Vec<Zval> {
        std::mem::take(&mut *self.garbage.borrow_mut())
    }

    /// Empties the slots after a driven call, however it ended.
    pub fn clear_slots(&self) {
        let requests = std::mem::take(&mut *self.requests.borrow_mut());
        let responses = std::mem::take(&mut *self.responses.borrow_mut());
        self.wakers.borrow_mut().clear();
        drop((requests, responses));
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
        let mut released = std::mem::take(&mut store.data_mut().values.released);
        released.append(&mut self.take_garbage());
        released
    }
}

struct Restore<'a>(&'a Cell<Active>, Active);

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        self.0.set(self.1);
    }
}

pub struct Parked<'a>(&'a StoreHandle, Active, bool);

impl Drop for Parked<'_> {
    fn drop(&mut self) {
        self.0.parked.set(self.2);
        self.0.active.set(self.1);
    }
}

pub const BUSY: &str = "the store is busy with a suspended call";

/// Thrown by anything that could run wasmtime's GC while a call is parked.
pub fn busy() -> PhpException {
    crate::error::runtime_error(wasmtime::Error::msg(BUSY))
}
