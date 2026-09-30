use ext_php_rs::convert::IntoZvalDyn;
use ext_php_rs::types::{ZendCallable, Zval};
use wasmtime::{AsContextMut, Caller, FuncType, Val, ValType};

use crate::store::{self, HostState};
use crate::throw;
use crate::value::{debug_type, from_val, to_val};

/// Wraps a PHP callable as a wasm function of type `ty`.
pub fn host_func(mut ctx: impl AsContextMut<Data = HostState>, ty: FuncType, callable: &Zval) -> wasmtime::Func {
    let key = ctx.as_context_mut().data_mut().values.insert_permanent(callable.shallow_clone());
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
    let callable = caller.data().values.get(key).shallow_clone();
    let args = {
        let mut ctx = caller.as_context_mut();
        params.iter().map(|param| from_val(&mut ctx, param)).collect::<Result<Vec<Zval>, _>>()?
    };

    let returned = store::current().enter_host(caller, || {
        let args: Vec<&dyn IntoZvalDyn> = args.iter().map(|arg| arg as &dyn IntoZvalDyn).collect();
        ZendCallable::new(&callable)?.try_call(args)
    });
    // A PHP exception stays pending in the engine while wasm unwinds, and the
    // PHP entry point that started the call rethrows it unchanged.
    let returned = match returned {
        Ok(returned) => returned,
        Err(err) => {
            let mut ctx = caller.as_context_mut();
            // A WasmThrow becomes a wasm exception that wasm code can catch.
            if let Some(exception) = throw::take_pending(&mut ctx) {
                return ctx.throw(exception?);
            }
            return Err(wasmtime::Error::msg(format!("PHP callback failed: {err}")));
        }
    };

    let mut ctx = caller.as_context_mut();
    match result_types {
        [] => {}
        [ty] => results[0] = to_val(&mut ctx, &returned, ty)?,
        types => {
            let list = returned.array().filter(|list| list.len() == types.len()).ok_or_else(|| {
                wasmtime::Error::msg(format!("expected a list of {} results, got {}", types.len(), debug_type(&returned)))
            })?;
            for ((slot, ty), value) in results.iter_mut().zip(types).zip(list.values()) {
                *slot = to_val(&mut ctx, value, ty)?;
            }
        }
    }
    Ok(())
}
