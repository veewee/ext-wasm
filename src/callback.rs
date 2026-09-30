use ext_php_rs::boxed::ZBox;
use ext_php_rs::convert::IntoZvalDyn;
use ext_php_rs::types::{ZendCallable, ZendObject, Zval};
use wasmtime::{AsContextMut, Caller, FuncType, Val, ValType};

use crate::store::{self, HostState};
use crate::suspend;
use crate::throw;
use crate::value::{debug_type, from_val, to_val};

/// Wraps a PHP callable as a wasm function of type `ty`.
pub fn host_func(
    mut ctx: impl AsContextMut<Data = HostState>,
    ty: FuncType,
    callable: &Zval,
    suspending: bool,
) -> wasmtime::Func {
    let key = ctx
        .as_context_mut()
        .data_mut()
        .values
        .insert_permanent(callable.shallow_clone());
    let result_types: Vec<ValType> = ty.results().collect();
    if ctx.as_context().data().is_async {
        return suspend::host_func(ctx, ty, key, result_types, suspending);
    }
    ctx.as_context_mut().data_mut().sync_callbacks = true;
    wasmtime::Func::new(ctx, ty, move |mut caller, params, results| {
        invoke(&mut caller, key, &result_types, params, results)
    })
}

fn invoke(
    caller: &mut Caller<'_, HostState>,
    key: usize,
    result_types: &[ValType],
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let store = store::of(caller);
    let callable = caller.data().values.get(key).shallow_clone();
    let args = {
        let mut ctx = caller.as_context_mut();
        params
            .iter()
            .map(|param| from_val(&mut ctx, param))
            .collect::<Result<Vec<Zval>, _>>()?
    };

    let returned = store.enter_host(caller, || {
        let args: Vec<&dyn IntoZvalDyn> = args.iter().map(|arg| arg as &dyn IntoZvalDyn).collect();
        let _no_fiber_switch = FiberSwitchBlock::new();
        ZendCallable::new(&callable)?.try_call(args)
    });

    let (outcome, thrown) = settle(
        caller,
        returned.as_ref().map_err(ToString::to_string),
        result_types,
        results,
    );
    let released: Vec<_> = thrown.into_iter().collect();

    // Releasing these can run PHP destructors, which may use wasm objects again.
    // The outer call still holds the store, so they run inside the host context.
    store.enter_host(caller, move || {
        let _no_fiber_switch = FiberSwitchBlock::new();
        drop((returned, args, callable, released));
    });
    outcome
}

/// Writes a callback's return value into `results`, or raises what it threw.
///
/// A WasmThrow becomes a wasm exception that wasm code can catch; its PHP
/// object is handed back for the caller to release. Any other PHP exception
/// stays pending in the engine while wasm unwinds, and the PHP entry point
/// that started the call rethrows it.
pub fn settle(
    caller: &mut Caller<'_, HostState>,
    returned: Result<&Zval, String>,
    result_types: &[ValType],
    results: &mut [Val],
) -> (wasmtime::Result<()>, Option<ZBox<ZendObject>>) {
    match returned {
        Ok(value) => (write_results(caller, value, result_types, results), None),
        Err(err) => {
            let mut ctx = caller.as_context_mut();
            match throw::take_pending(&mut ctx) {
                Some((object, exception)) => (
                    exception.and_then(|exception| ctx.throw(exception)),
                    Some(object),
                ),
                None => (
                    Err(wasmtime::Error::msg(format!("PHP callback failed: {err}"))),
                    None,
                ),
            }
        }
    }
}

unsafe extern "C" {
    fn zend_fiber_switch_block();
    fn zend_fiber_switch_unblock();
}

/// Makes `Fiber::suspend()`, `resume()` and `start()` throw a FiberError while
/// a callback runs, as PHP does for destructors during garbage collection.
///
/// wasmtime requires calls into wasm to return in the order they started. A
/// callback that suspends its fiber lets another fiber call into wasm and
/// return first, which aborts the process inside wasmtime.
pub(crate) struct FiberSwitchBlock;

impl FiberSwitchBlock {
    pub(crate) fn new() -> Self {
        // SAFETY: a counter in the executor globals, balanced by `drop`.
        unsafe { zend_fiber_switch_block() };
        Self
    }
}

impl Drop for FiberSwitchBlock {
    fn drop(&mut self) {
        // SAFETY: undoes the `zend_fiber_switch_block` call of `new`.
        unsafe { zend_fiber_switch_unblock() };
    }
}

fn write_results(
    caller: &mut Caller<'_, HostState>,
    returned: &Zval,
    result_types: &[ValType],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let mut ctx = caller.as_context_mut();
    match result_types {
        [] => {}
        [ty] => results[0] = to_val(&mut ctx, returned, ty)?,
        types => {
            let list = returned
                .array()
                .filter(|list| list.len() == types.len())
                .ok_or_else(|| {
                    wasmtime::Error::msg(format!(
                        "expected a list of {} results, got {}",
                        types.len(),
                        debug_type(returned)
                    ))
                })?;
            for ((slot, ty), value) in results.iter_mut().zip(types).zip(list.values()) {
                *slot = to_val(&mut ctx, value, ty)?;
            }
        }
    }
    Ok(())
}
