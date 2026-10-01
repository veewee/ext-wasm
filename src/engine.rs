use std::sync::{Arc, Mutex, OnceLock};

use rayon::{ThreadPool, ThreadPoolBuildError, ThreadPoolBuilder};

use ext_php_rs::zend::ExecutorGlobals;
use wasmtime::{Cache, CacheConfig, Config, Engine};

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
            .wasm_exceptions(true)
            .wasm_component_model_map(true)
            .wasm_component_model_error_context(true)
            // Off unless asked for: wasmtime then captures a dump for every
            // error leaving wasm, throwing imports and exits included.
            .coredump_on_trap(!crate::coredump::directory().is_empty());
        // Mach exception ports do not survive fork(), which PHP-FPM and pcntl rely on.
        #[cfg(target_os = "macos")]
        config.macos_use_mach_ports(false);
        config.cache(compilation_cache());
        Engine::new(&config).expect("the wasmtime engine configuration is valid")
    })
}

/// The on-disk cache of compiled machine code, configured by `wasm.cache` and
/// `wasm.cache_dir`. Like a browser's code cache it is keyed by the module
/// bytes and the engine settings, so a changed module or engine never gets a
/// stale entry. A directory that cannot be used disables the cache rather than
/// failing compilation.
fn compilation_cache() -> Option<Cache> {
    let settings = ExecutorGlobals::get().ini_values();
    let setting = |name: &str| settings.get(name).cloned().flatten().unwrap_or_default();
    if !matches!(
        setting("wasm.cache").to_ascii_lowercase().as_str(),
        "1" | "on" | "yes" | "true"
    ) {
        return None;
    }
    let mut config = CacheConfig::new();
    let directory = setting("wasm.cache_dir");
    if !directory.is_empty() {
        config.with_directory(directory);
    }
    Cache::new(config).ok()
}

/// Runs `compile` on a rayon pool whose threads exist in this process.
///
/// wasmtime compiles functions in parallel on rayon's global pool. After
/// `fork()` the child inherits that pool's bookkeeping but not its threads, so
/// its next compile waits forever. Prefork servers compile in the parent and
/// fork workers afterwards, so a forked child compiles on a pool of its own.
/// The process that compiled first keeps the global pool, which compiles large
/// modules about a third faster than a separate pool in measurements.
pub fn compile_in_process_pool<R: Send>(
    compile: impl FnOnce() -> R + Send,
) -> Result<R, ThreadPoolBuildError> {
    static GLOBAL_POOL_OWNER: OnceLock<u32> = OnceLock::new();
    static FORKED_POOL: Mutex<Option<(u32, Arc<ThreadPool>)>> = Mutex::new(None);

    let pid = std::process::id();
    if *GLOBAL_POOL_OWNER.get_or_init(|| pid) == pid {
        return Ok(compile());
    }

    let pool = {
        let mut slot = FORKED_POOL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match slot.as_ref() {
            Some((owner, pool)) if *owner == pid => pool.clone(),
            _ => {
                // A pool inherited from a forking parent has no threads, and
                // dropping it could block on them, so it is leaked instead.
                if let Some(inherited) = slot.take() {
                    std::mem::forget(inherited);
                }
                // Falling back to the inherited pool would hang, so a pool
                // that cannot be built is an error for this compile.
                let pool = Arc::new(ThreadPoolBuilder::new().build()?);
                *slot = Some((pid, pool.clone()));
                pool
            }
        }
    };

    Ok(pool.install(compile))
}

/// The tokio runtime WASI file streams and clocks run on, one per process.
///
/// wasmtime-wasi otherwise uses a global runtime of its own. A forked child
/// inherits that runtime without its worker threads, so its first file read
/// or timer waited forever when the parent had used WASI before forking. The
/// runtime wasmtime-wasi finds entered on the current thread takes its place.
pub fn wasi_runtime() -> tokio::runtime::Handle {
    thread_local! {
        static CACHED: std::cell::RefCell<Option<(u32, tokio::runtime::Handle)>> =
            const { std::cell::RefCell::new(None) };
    }
    let pid = std::process::id();
    CACHED.with(|cached| {
        let mut cached = cached.borrow_mut();
        match cached.as_ref() {
            Some((owner, handle)) if *owner == pid => handle.clone(),
            _ => {
                let handle = process_runtime(pid);
                *cached = Some((pid, handle.clone()));
                handle
            }
        }
    })
}

fn process_runtime(pid: u32) -> tokio::runtime::Handle {
    static RUNTIME: Mutex<Option<(u32, tokio::runtime::Runtime)>> = Mutex::new(None);

    let mut slot = RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((owner, runtime)) = slot.as_ref()
        && *owner == pid
    {
        return runtime.handle().clone();
    }
    // Dropping an inherited runtime would wait for threads that do not exist here.
    if let Some(inherited) = slot.take() {
        std::mem::forget(inherited);
    }
    // Multi-threaded, because a current-thread runtime only drives its timers
    // from Runtime::block_on, not from the Handle::block_on wasmtime-wasi uses.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .enable_io()
        .build()
        .expect("a tokio runtime can be built");
    let handle = runtime.handle().clone();
    *slot = Some((pid, runtime));
    handle
}
