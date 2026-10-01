use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ext_php_rs::zend::ExecutorGlobals;
use wasmtime::{StoreContextMut, WasmCoreDump};

use crate::store::HostState;

/// `wasm.coredump_dir`, empty when coredumps are off.
pub fn directory() -> String {
    ExecutorGlobals::get()
        .ini_values()
        .get("wasm.coredump_dir")
        .cloned()
        .flatten()
        .unwrap_or_default()
}

/// Writes the coredump a trap carries to `wasm.coredump_dir`, and gives the
/// line for the exception message. `None` when the error carries no dump: no
/// wasm ran, or coredumps are off.
///
/// Runs before the store is used again, because the dump reads memory from
/// the store when it is serialized, not when the trap happened.
pub fn write(ctx: &mut StoreContextMut<'_, HostState>, err: &wasmtime::Error) -> Option<String> {
    let dump = err.downcast_ref::<WasmCoreDump>()?;
    let directory = directory();
    if directory.is_empty() {
        return None;
    }
    // Rust resolves a relative path against the process working directory,
    // which is not PHP's in ZTS builds.
    if !Path::new(&directory).is_absolute() {
        return Some("coredump not written: wasm.coredump_dir must be an absolute path".into());
    }
    let bytes = dump.serialize(&mut *ctx, "php");
    Some(match save(Path::new(&directory), &bytes) {
        Ok(path) => format!("coredump: {}", path.display()),
        Err(err) => format!("coredump not written to {directory}: {err}"),
    })
}

fn save(directory: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    static WRITTEN: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let path = directory.join(format!(
        "wasm-{}-{millis}-{}.coredump",
        std::process::id(),
        WRITTEN.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    // A dump holds the whole linear memory, which may contain secrets.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path)?;
    if let Err(err) = file.write_all(bytes) {
        // A full disk would otherwise keep a truncated dump for every trap.
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(err);
    }
    Ok(path)
}
