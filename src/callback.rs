use ext_php_rs::convert::IntoZvalDyn;
use ext_php_rs::types::{ZendCallable, Zval};
use wasmtime::{AsContextMut, Caller, FuncType, Val, ValType};

use crate::store::{self, HostState};
use crate::throw;
use crate::value::{debug_type, from_val, to_val};

/// Wraps a PHP callable as a wasm function of type `ty`.
pub fn host_func(
    mut ctx: impl AsContextMut<Data = HostState>,
    ty: FuncType,
    callable: &Zval,
) -> wasmtime::Func {
    let key = ctx
        .as_context_mut()
        .data_mut()
        .values
        .insert_permanent(callable.shallow_clone());
    let result_types: Vec<ValType> = ty.results().collect();
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
    let store = store::current();
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

    let mut released = Vec::new();
    let outcome = match &returned {
        Ok(value) => write_results(caller, value, result_types, results),
        Err(err) => {
            let mut ctx = caller.as_context_mut();
            // A WasmThrow becomes a wasm exception that wasm code can catch.
            // Any other PHP exception stays pending in the engine while wasm
            // unwinds, and the PHP entry point that started the call rethrows it.
            match throw::take_pending(&mut ctx) {
                Some((object, exception)) => {
                    released.push(object);
                    exception.and_then(|exception| ctx.throw(exception))
                }
                None => Err(wasmtime::Error::msg(format!("PHP callback failed: {err}"))),
            }
        }
    };

    // Releasing these can run PHP destructors, which may use wasm objects again.
    // The outer call still holds the store, so they run inside the host context.
    store.enter_host(caller, move || drop((returned, args, callable, released)));
    outcome
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
struct FiberSwitchBlock;

impl FiberSwitchBlock {
    fn new() -> Self {
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
