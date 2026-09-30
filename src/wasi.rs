// `outputLimit` is camel case because ext-php-rs uses the Rust name as the PHP
// parameter name, and its macro expands the name outside the function.
#![allow(non_snake_case)]

use std::cell::Cell;
use std::rc::Rc;

use ext_php_rs::binary::Binary;
use ext_php_rs::binary_slice::BinarySlice;
use ext_php_rs::boxed::ZBox;
use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ArrayKey, ZendHashTable, Zval};
use wasmtime::Linker;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::{FsPerms, I32Exit, WasiCtxBuilder, p1};

use crate::engine::engine;
use crate::error::{error, runtime_error, type_error, value_error};
use crate::func::Func;
use crate::instance::Instance;
use crate::store::{self, HostState, SharedStore};
use crate::throw::call_error;

const DEFAULT_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

/// A WASI preview1 environment for one run of one module, like Node's `WASI`.
///
/// Nothing of the host is visible to the module except what is passed here:
/// no environment, no stdio and no files outside the preopened directories.
/// stdout and stderr are captured and read after the run.
#[php_class]
#[php(name = "Wasm\\Wasi")]
#[php(flags = ClassFlags::Final)]
pub struct Wasi {
    store: SharedStore,
    imports: ZBox<ZendHashTable>,
    stdout: MemoryOutputPipe,
    stderr: MemoryOutputPipe,
    output_limit: usize,
    used: Cell<bool>,
}

#[php_impl]
impl Wasi {
    /// @param list<string>|null $args argv, including the program name
    /// @param array<string, string>|null $env
    /// @param array<string, string|array{path: string, writable?: bool}>|null $preopens guest path => host path
    /// @param int|null $outputLimit bytes kept of stdout and of stderr, 16 MiB by default
    pub fn __construct(
        args: Option<Vec<String>>,
        env: Option<&ZendHashTable>,
        preopens: Option<&ZendHashTable>,
        stdin: Option<BinarySlice<u8>>,
        outputLimit: Option<i64>,
    ) -> PhpResult<Self> {
        let output_limit = match outputLimit {
            None => DEFAULT_OUTPUT_LIMIT,
            Some(limit) => usize::try_from(limit)
                .map_err(|_| value_error("outputLimit must not be negative"))?,
        };
        // One byte of headroom tells a full buffer from one that overflowed; see `check_output`.
        let stdout = MemoryOutputPipe::new(output_limit + 1);
        let stderr = MemoryOutputPipe::new(output_limit + 1);
        let mut builder = WasiCtxBuilder::new();
        // File access otherwise waits for tokio worker threads, which a forked
        // child does not have. Preopens copy this flag, so it is set first.
        builder.allow_blocking_current_thread(true);
        builder.args(&args.unwrap_or_default());
        for (key, value) in env.map(ZendHashTable::iter).into_iter().flatten() {
            let value = value
                .str()
                .ok_or_else(|| type_error(format!("env value of \"{key}\" must be a string")))?;
            builder.env(key.to_string(), value);
        }
        for (guest, spec) in preopens.map(ZendHashTable::iter).into_iter().flatten() {
            if let ArrayKey::Long(_) = guest {
                return Err(type_error(
                    "preopens must map guest paths to host paths, like ['/data' => '/srv/data']",
                ));
            }
            let (host, writable) = match spec.array() {
                Some(options) => (
                    options.get("path").and_then(Zval::str).ok_or_else(|| {
                        type_error(format!("preopen \"{guest}\" needs a \"path\""))
                    })?,
                    options
                        .get("writable")
                        .and_then(Zval::bool)
                        .unwrap_or(false),
                ),
                None => (
                    spec.str().ok_or_else(|| {
                        type_error(format!(
                            "preopen \"{guest}\" must be a host path or an array with \"path\""
                        ))
                    })?,
                    false,
                ),
            };
            let perms = if writable {
                FsPerms::ReadWrite
            } else {
                FsPerms::ReadOnly
            };
            builder
                .preopened_dir(host, guest.to_string(), perms)
                .map_err(|err| value_error(format!("cannot preopen {host}: {err:#}")))?;
        }
        builder.stdin(MemoryInputPipe::new(
            stdin.map(|bytes| bytes.to_vec()).unwrap_or_default(),
        ));
        builder.stdout(stdout.clone()).stderr(stderr.clone());

        let store = store::new();
        store.with(|mut ctx| ctx.data_mut().wasi = Some(builder.build_p1()));
        let imports = import_object(&store)?;
        Ok(Self {
            store,
            imports,
            stdout,
            stderr,
            output_limit,
            used: Cell::new(false),
        })
    }

    /// @return array{wasi_snapshot_preview1: array<string, \Wasm\Func>}
    pub fn get_import_object(&self) -> ZBox<ZendHashTable> {
        self.imports.clone()
    }

    /// Runs `_start` and returns the exit code.
    pub fn start(&self, instance: &Instance) -> PhpResult<i64> {
        let func = self
            .entry(instance, "_start")?
            .ok_or_else(|| type_error("the instance has no _start export"))?;
        self.used.set(true);
        let code = self
            .store
            .with(|mut ctx| match func.call(&mut ctx, &[], &mut []) {
                Ok(()) => Ok(0),
                Err(err) => match err.downcast_ref::<I32Exit>() {
                    Some(exit) => Ok(i64::from(exit.0)),
                    None => Err(call_error(&mut ctx, err)),
                },
            })?;
        self.check_output()?;
        Ok(code)
    }

    /// Runs `_initialize` when the module exports it, for modules used as a library.
    // Renamed in Rust because the class macro generates an `initialize` of its own.
    #[php(name = "initialize")]
    pub fn initialize_reactor(&self, instance: &Instance) -> PhpResult<()> {
        let entry = self.entry(instance, "_initialize")?;
        self.used.set(true);
        let Some(func) = entry else {
            return Ok(());
        };
        self.store.with(|mut ctx| {
            func.call(&mut ctx, &[], &mut [])
                .map_err(|err| call_error(&mut ctx, err))
        })?;
        self.check_output()
    }

    pub fn stdout(&self) -> Binary<u8> {
        self.captured(&self.stdout)
    }

    pub fn stderr(&self) -> Binary<u8> {
        self.captured(&self.stderr)
    }
}

impl Wasi {
    /// The entry point `name` of an instance in this Wasi's store.
    fn entry(&self, instance: &Instance, name: &str) -> PhpResult<Option<wasmtime::Func>> {
        let exports = instance.exports_object();
        if !Rc::ptr_eq(exports.store(), &self.store) {
            return Err(store::mismatch("Instance"));
        }
        if self.used.get() {
            return Err(error(
                "a Wasm\\Wasi object runs one module once; create a new one",
            ));
        }
        Ok(exports.func(name))
    }

    /// The pipes answer writes past the limit with an I/O error the program
    /// may ignore, so an overflow is reported once the call returns.
    fn check_output(&self) -> PhpResult<()> {
        if [&self.stdout, &self.stderr]
            .iter()
            .any(|pipe| pipe.contents().len() > self.output_limit)
        {
            return Err(runtime_error(wasmtime::Error::msg(format!(
                "wasm program output exceeded the limit of {} bytes",
                self.output_limit
            ))));
        }
        Ok(())
    }

    fn captured(&self, pipe: &MemoryOutputPipe) -> Binary<u8> {
        let contents = pipe.contents();
        contents[..contents.len().min(self.output_limit)]
            .to_vec()
            .into()
    }
}

fn import_object(store: &SharedStore) -> PhpResult<ZBox<ZendHashTable>> {
    let mut linker: Linker<HostState> = Linker::new(engine());
    p1::add_to_linker_sync(&mut linker, |state: &mut HostState| {
        state
            .wasi
            .as_mut()
            .expect("WASI functions only exist in stores created by Wasm\\Wasi")
    })
    .map_err(|err| error(format!("{err:#}")))?;
    let functions: Vec<(String, wasmtime::Func)> = store.with(|mut ctx| {
        linker
            .iter(&mut ctx)
            .filter_map(|(_, name, ext)| ext.into_func().map(|func| (name.to_string(), func)))
            .collect()
    });
    let mut namespace = ZendHashTable::new();
    for (name, inner) in functions {
        let func = Func {
            store: store.clone(),
            inner,
        };
        namespace.insert(name.as_str(), func.into_zval(false)?)?;
    }
    let mut object = ZendHashTable::new();
    object.insert("wasi_snapshot_preview1", namespace)?;
    Ok(object)
}
