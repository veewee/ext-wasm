// `outputLimit` is camel case because ext-php-rs uses the Rust name as the PHP
// parameter name, and its macro expands the name outside the function.
#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
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
use wasmtime::component::{ResourceTable, Val};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::{FsPerms, I32Exit, WasiCtxBuilder, p1};

use crate::component::http::{self, HostRule, WasiHttp};
use crate::component::instance::Instance as ComponentInstance;
use crate::component::sockets;
use crate::engine::engine;
use crate::error::{error, runtime_error, type_error, value_error};
use crate::func::{self, Func};
use crate::instance::Instance;
use crate::store::{self, HostState, SharedStore, WasiP2};
use crate::throw::call_error;
use crate::value::{debug_type, downcast};

const DEFAULT_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

/// A WASI environment for one run of one module or component, like Node's `WASI`.
///
/// Nothing of the host is visible to the program except what is passed here:
/// no environment, no stdio and no files outside the preopened directories.
/// stdout and stderr are captured and read after the run. A core module gets
/// WASI preview1 through `getImportObject()`, a component gets preview2 when
/// the Wasi object is passed to `Wasm\Component\Instance`.
#[php_class]
#[php(name = "Wasm\\Wasi")]
#[php(flags = ClassFlags::Final)]
pub struct Wasi {
    store: SharedStore,
    /// Built on first use, as preview1 or preview2, because it builds once.
    builder: RefCell<Option<WasiCtxBuilder>>,
    imports: RefCell<Option<ZBox<ZendHashTable>>>,
    /// The store of the component instance this object was given to.
    component: RefCell<Option<SharedStore>>,
    stdout: MemoryOutputPipe,
    stderr: MemoryOutputPipe,
    output_limit: usize,
    used: Cell<bool>,
    /// The hosts a component may send HTTP requests to; `None` links no wasi:http.
    http_hosts: Option<Vec<HostRule>>,
}

#[php_impl]
impl Wasi {
    /// @param list<string>|null $args argv, including the program name
    /// @param array<string, string>|null $env
    /// @param array<string, string|array{path: string, writable?: bool}>|null $preopens guest path => host path
    /// @param int|null $outputLimit bytes kept of stdout and of stderr, 16 MiB by default
    /// @param list<string>|null $httpHosts hosts a component may send HTTP requests to: "host", "host:port" or "*.domain"; checked by name, not by the address it resolves to
    /// @param list<string>|null $tcpHosts destinations a component may open TCP connections to: "host:port", "ip:port" or "network/prefix:port", with * for any port; a host is checked by the addresses it resolves to when the component connects
    /// @param list<string>|null $udpHosts destinations a component may send UDP datagrams to and receive them from, in the same form; a host is resolved once, when this object is created
    // Each parameter is a PHP named argument, so grouping them would change the PHP API.
    #[allow(clippy::too_many_arguments)]
    pub fn __construct(
        args: Option<Vec<String>>,
        env: Option<&ZendHashTable>,
        preopens: Option<&ZendHashTable>,
        stdin: Option<BinarySlice<u8>>,
        outputLimit: Option<i64>,
        httpHosts: Option<&ZendHashTable>,
        tcpHosts: Option<&ZendHashTable>,
        udpHosts: Option<&ZendHashTable>,
    ) -> PhpResult<Self> {
        let http_hosts = httpHosts.map(http::parse_hosts).transpose()?;
        let tcp_hosts = tcpHosts
            .map(|hosts| sockets::parse_hosts(hosts, "tcpHosts"))
            .transpose()?;
        let udp_hosts = udpHosts
            .map(|hosts| sockets::parse_hosts(hosts, "udpHosts"))
            .transpose()?;
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
        if tcp_hosts.is_some() || udp_hosts.is_some() {
            sockets::allow(&mut builder, tcp_hosts, udp_hosts, socket_timeout());
        }
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

        Ok(Self {
            store: store::new(),
            builder: RefCell::new(Some(builder)),
            imports: RefCell::new(None),
            component: RefCell::new(None),
            stdout,
            stderr,
            output_limit,
            used: Cell::new(false),
            http_hosts,
        })
    }

    /// The preview1 functions for a core module.
    ///
    /// @return array{wasi_snapshot_preview1: array<string, \Wasm\Func>}
    pub fn get_import_object(&self) -> PhpResult<ZBox<ZendHashTable>> {
        if let Some(imports) = self.imports.borrow().as_ref() {
            return Ok(imports.clone());
        }
        let mut builder = self.take_builder()?;
        self.store
            .with(|mut ctx| ctx.data_mut().wasi = Some(builder.build_p1()));
        let imports = import_object(&self.store)?;
        *self.imports.borrow_mut() = Some(imports.clone());
        Ok(imports)
    }

    /// Runs `_start` of a module, or `wasi:cli/run` of a component, and
    /// returns the exit code.
    ///
    /// @param \Wasm\Instance|\Wasm\Component\Instance $instance
    pub fn start(&self, instance: &Zval) -> PhpResult<i64> {
        if let Some(component) = downcast::<ComponentInstance>(instance) {
            return self.start_component(component);
        }
        let instance = downcast::<Instance>(instance).ok_or_else(|| {
            type_error(format!(
                "Wasm\\Wasi::start(): Argument #1 ($instance) must be of type Wasm\\Instance|Wasm\\Component\\Instance, {} given",
                debug_type(instance)
            ))
        })?;
        let func = self
            .entry(instance, "_start")?
            .ok_or_else(|| type_error("the instance has no _start export"))?;
        self.used.set(true);
        let code = self.store.with(|mut ctx| {
            match func::run(&self.store, &mut ctx, &func, &[], &mut []) {
                Ok(()) => Ok(0),
                Err(err) => match err.downcast_ref::<I32Exit>() {
                    Some(exit) => Ok(i64::from(exit.0)),
                    None => Err(call_error(&mut ctx, err)),
                },
            }
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
            func::run(&self.store, &mut ctx, &func, &[], &mut [])
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

const ONE_RUN: &str = "a Wasm\\Wasi object runs one module once; create a new one";

impl Wasi {
    fn take_builder(&self) -> PhpResult<WasiCtxBuilder> {
        self.builder
            .borrow_mut()
            .take()
            .ok_or_else(|| error(ONE_RUN))
    }

    /// Gives the preview2 context to the store of a new component instance.
    pub fn attach(&self, store: &SharedStore) -> PhpResult<()> {
        let mut builder = self.take_builder()?;
        let wasi = WasiP2 {
            ctx: builder.build(),
            table: ResourceTable::new(),
        };
        let http = self
            .http_hosts
            .clone()
            .map(|rules| WasiHttp::new(rules, socket_timeout()));
        store.with(|mut ctx| {
            let state = ctx.data_mut();
            state.wasi_p2 = Some(wasi);
            state.http = http;
        });
        *self.component.borrow_mut() = Some(store.clone());
        Ok(())
    }

    /// Whether components given this object get wasi:http.
    pub fn allows_http(&self) -> bool {
        self.http_hosts.is_some()
    }

    fn start_component(&self, instance: &ComponentInstance) -> PhpResult<i64> {
        let store = self
            .component
            .borrow()
            .clone()
            .filter(|store| Rc::ptr_eq(store, instance.store()))
            .ok_or_else(|| store::mismatch("Instance"))?;
        if store.is_parked() {
            return Err(store::busy());
        }
        if self.used.get() {
            return Err(error(ONE_RUN));
        }
        let run = instance
            .func("wasi:cli/run", "run")
            .ok_or_else(|| type_error("the component exports no wasi:cli/run"))?;
        self.used.set(true);
        let code = store.with(|mut ctx| {
            let mut results = [Val::Bool(false)];
            match crate::component::func::run(&store, &mut ctx, run, &[], &mut results) {
                Ok(()) => Ok(match results[0] {
                    Val::Result(Ok(_)) => 0,
                    _ => 1,
                }),
                Err(err) => match err.downcast_ref::<I32Exit>() {
                    Some(exit) => Ok(i64::from(exit.0)),
                    None => Err(call_error(&mut ctx, err)),
                },
            }
        })?;
        self.check_output()?;
        Ok(code)
    }

    /// The entry point `name` of an instance in this Wasi's store.
    fn entry(&self, instance: &Instance, name: &str) -> PhpResult<Option<wasmtime::Func>> {
        if self.store.is_parked() {
            return Err(store::busy());
        }
        let exports = instance.exports_object();
        if !Rc::ptr_eq(exports.store(), &self.store) {
            return Err(store::mismatch("Instance"));
        }
        if self.used.get() {
            return Err(error(ONE_RUN));
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

/// PHP's default_socket_timeout, which also bounds the requests of components.
pub(crate) fn socket_timeout() -> Option<std::time::Duration> {
    let settings = ext_php_rs::zend::ExecutorGlobals::get().ini_values();
    let seconds: f64 = settings
        .get("default_socket_timeout")
        .cloned()
        .flatten()
        .and_then(|value| value.parse().ok())
        .unwrap_or(60.0);
    // A negative or zero timeout means no limit in PHP.
    (seconds > 0.0).then(|| std::time::Duration::from_secs_f64(seconds))
}
