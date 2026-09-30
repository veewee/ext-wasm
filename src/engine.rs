use std::sync::{Arc, Mutex, OnceLock};

use rayon::{ThreadPool, ThreadPoolBuilder};

use wasmtime::{Config, Engine};

/// One engine per process. Created on first use rather than at MINIT so that
/// wasmtime installs its signal handlers after PHP and other extensions have
/// installed theirs, and chains to them for faults outside wasm code.
pub fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::new();
        config
            .wasm_reference_types(true)
            .wasm_function_references(true)
            .wasm_gc(true)
            .wasm_simd(true)
            .wasm_exceptions(true);
        // Mach exception ports do not survive fork(), which PHP-FPM and pcntl rely on.
        #[cfg(target_os = "macos")]
        config.macos_use_mach_ports(false);
        Engine::new(&config).expect("the wasmtime engine configuration is valid")
    })
}

/// Runs `compile` on a rayon pool whose threads exist in this process.
///
/// wasmtime compiles functions in parallel on rayon's global pool. After
/// `fork()` the child inherits that pool's bookkeeping but not its threads, so
/// its next compile waits forever. Prefork servers compile in the parent and
/// fork workers afterwards, so a forked child compiles on a pool of its own.
/// The process that compiled first keeps the global pool, which compiles large
/// modules about a third faster than a separate pool in measurements.
pub fn compile_in_process_pool<R: Send>(compile: impl FnOnce() -> R + Send) -> R {
    static GLOBAL_POOL_OWNER: OnceLock<u32> = OnceLock::new();
    static FORKED_POOL: Mutex<Option<(u32, Arc<ThreadPool>)>> = Mutex::new(None);

    let pid = std::process::id();
    if *GLOBAL_POOL_OWNER.get_or_init(|| pid) == pid {
        return compile();
    }

    let pool = {
        let mut slot = FORKED_POOL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match slot.as_ref() {
            Some((owner, pool)) if *owner == pid => Some(pool.clone()),
            _ => {
                // A pool inherited from a forking parent has no threads, and
                // dropping it could block on them, so it is leaked instead.
                if let Some(inherited) = slot.take() {
                    std::mem::forget(inherited);
                }
                let pool = ThreadPoolBuilder::new().build().ok().map(Arc::new);
                *slot = pool.clone().map(|pool| (pid, pool));
                pool
            }
        }
    };

    match pool {
        Some(pool) => pool.install(compile),
        None => compile(),
    }
}
