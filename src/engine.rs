use std::sync::OnceLock;

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
