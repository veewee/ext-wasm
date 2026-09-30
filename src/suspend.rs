use std::future::Future;
use std::pin::{Pin, pin};
use std::task::{Context, Poll, Waker};

use ext_php_rs::convert::{IntoZval, IntoZvalDyn};
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendCallable, Zval};
use wasmtime::{AsContextMut, Caller, FuncType, Val, ValType};

use crate::callback::{FiberSwitchBlock, settle};
use crate::component::imports::{Target, call_target};
use crate::error::type_error;
use crate::store::{self, Active, HostState, StoreHandle};
use crate::value::{debug_type, from_val};

/// Marks a function import that may suspend the calling Fiber, like JS
/// `WebAssembly.Suspending`.
///
/// Every other Fiber keeps running while wasm waits for the callback. Calling
/// into the instance's store again before the callback returns throws a
/// RuntimeError, so run one instance per Fiber.
#[php_class]
#[php(name = "Wasm\\Suspending")]
#[php(flags = ClassFlags::Final)]
pub struct Suspending {
    pub callback: Zval,
}

#[php_impl]
impl Suspending {
    pub fn __construct(callback: &Zval) -> PhpResult<Self> {
        if !callback.is_callable() {
            return Err(type_error(format!(
                "Wasm\\Suspending::__construct(): Argument #1 ($callback) must be a valid callback, {} given",
                debug_type(callback)
            )));
        }
        Ok(Self {
            callback: callback.shallow_clone(),
        })
    }
}

/// A PHP callback that wasm is waiting for.
pub struct Request {
    /// The store access of the waiting call, for PHP code the callback runs.
    pub access: Active,
    pub callee: Callee,
    pub args: Vec<Zval>,
    pub suspending: bool,
}

/// What a waiting call asks PHP to run.
pub enum Callee {
    /// A core import.
    Callable(Zval),
    /// A component import: a callable or a method of a resource's PHP class.
    Component(Target, Zval),
}

/// What the callback returned, or the message of its failure. An exception
/// it threw stays pending in the engine.
pub type Response = Result<Zval, String>;

/// Wraps a callable already stored under `key` as an async wasm function.
///
/// PHP code cannot run on wasmtime's async stack, whose addresses fail PHP's
/// stack limit check, so the function only asks `drive` to run the callback
/// and waits for the answer.
pub fn async_host_func(
    ctx: impl AsContextMut<Data = HostState>,
    ty: FuncType,
    key: usize,
    result_types: Vec<ValType>,
    suspending: bool,
) -> wasmtime::Func {
    wasmtime::Func::new_async(ctx, ty, move |caller, params, results| {
        Box::new(HostCall {
            caller,
            params,
            results,
            key,
            suspending,
            result_types: result_types.clone(),
            requested: false,
        })
    })
}

/// Holds only wasm values, so it is Send and never drops a Zval on
/// wasmtime's stack, where a destructor would fail PHP's stack check.
struct HostCall<'a> {
    caller: Caller<'a, HostState>,
    params: &'a [Val],
    results: &'a mut [Val],
    key: usize,
    suspending: bool,
    result_types: Vec<ValType>,
    requested: bool,
}

impl Future for HostCall<'_> {
    type Output = wasmtime::Result<()>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        // Taken anew on each poll: an Rc held across Pending would make the future !Send.
        let store = store::of(&this.caller);
        if !this.requested {
            // Converted here, on the wasm fiber, where a GC these allocations
            // start still sees the wasm frames.
            let mut ctx = this.caller.as_context_mut();
            let mut args = Vec::with_capacity(this.params.len());
            for param in this.params {
                match from_val(&mut ctx, param) {
                    Ok(arg) => args.push(arg),
                    Err(err) => {
                        for arg in args {
                            ctx.data_mut().values.release(arg);
                        }
                        return Poll::Ready(Err(err.into()));
                    }
                }
            }
            let callable = ctx.data().values.get(this.key).shallow_clone();
            store.put_request(Request {
                // The future is pinned inside wasmtime, so this address holds until it is dropped.
                access: Active::Core((&mut this.caller as *mut Caller<'_, HostState>).cast()),
                callee: Callee::Callable(callable),
                args,
                suspending: this.suspending,
            });
            this.requested = true;
            return Poll::Pending;
        }

        let Some(returned) = store.take_response() else {
            return Poll::Ready(Err(wasmtime::Error::msg(
                "wasm resumed a PHP callback that has not returned",
            )));
        };
        let (outcome, thrown) = settle(
            &mut this.caller,
            returned.as_ref().map_err(Clone::clone),
            &this.result_types,
            this.results,
        );
        if let Ok(value) = returned {
            store.put_garbage(value);
        }
        if let Some(object) = thrown.and_then(|object| object.into_zval(false).ok()) {
            store.put_garbage(object);
        }
        Poll::Ready(outcome)
    }
}

/// Runs an async call into the store to completion on the PHP stack, calling
/// the PHP callbacks it waits for in between.
///
/// A Suspending callback may suspend the current Fiber here; the call stays
/// parked, with its store busy, until the Fiber resumes.
pub fn drive<R>(
    store: &StoreHandle,
    future: impl Future<Output = wasmtime::Result<R>>,
) -> wasmtime::Result<R> {
    let _clear = ClearSlots(store);
    // Declared after the guard, so the future is dropped first: wasmtime then
    // unwinds a parked call before the slots and the caller pointer go.
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    let runtime = store.uses_wasi().then(crate::engine::wasi_runtime);
    loop {
        // Entered for this poll only: the PHP callback between polls may
        // suspend the Fiber, and another Fiber's guard may come and go meanwhile.
        let polled = {
            let _entered = runtime.as_ref().map(tokio::runtime::Handle::enter);
            future.as_mut().poll(&mut cx)
        };
        if let Poll::Ready(result) = polled {
            return result;
        }
        // Without a request wasmtime only yielded, for example inside its GC.
        if let Some(request) = store.take_request() {
            call_parked(store, request);
        }
    }
}

/// Runs the callback of a parked call and leaves its outcome for the next poll.
///
/// When the callback leaves exit() or a destroyed Fiber's graceful exit
/// pending, that poll fails the wasm call without running PHP, and the entry
/// point's own error is not thrown over the pending one, so PHP keeps unwinding.
fn call_parked(store: &StoreHandle, request: Request) {
    let Request {
        access,
        callee,
        args,
        suspending,
    } = request;
    let _parked = store.park(access);
    let block = || (!suspending).then(FiberSwitchBlock::new);
    {
        // What the previous callback returned. Releasing it can run PHP
        // destructors, which may use wasm objects again.
        let _no_fiber_switch = block();
        drop(store.take_garbage());
    }
    let returned = {
        let _no_fiber_switch = block();
        match &callee {
            Callee::Callable(callable) => {
                let args: Vec<&dyn IntoZvalDyn> =
                    args.iter().map(|arg| arg as &dyn IntoZvalDyn).collect();
                ZendCallable::new(callable).and_then(|callable| callable.try_call(args))
            }
            Callee::Component(target, callable) => call_target(target, callable, &args),
        }
    };
    {
        // Releasing these can run PHP destructors, which may use wasm objects again.
        let _no_fiber_switch = block();
        drop((args, callee));
    }
    store.put_response(returned.map_err(|err| err.to_string()));
}

struct ClearSlots<'a>(&'a StoreHandle);

impl Drop for ClearSlots<'_> {
    fn drop(&mut self) {
        self.0.clear_slots();
    }
}
