# WebAssembly for PHP

ext-wasm runs WebAssembly modules inside PHP. The API follows the [JavaScript WebAssembly API](https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface), so anything you know from `WebAssembly.Module`, `Instance`, `Memory`, `Table`, `Global` and `Tag` carries over. Under the hood it uses [wasmtime](https://wasmtime.dev) through [ext-php-rs](https://github.com/extphprs/ext-php-rs).

```php
$instance = new Wasm\Instance(new Wasm\Module(<<<'WAT'
    (module
      (import "env" "log" (func $log (param i32)))
      (func (export "add") (param i32 i32) (result i32)
        (call $log (local.get 0))
        (i32.add (local.get 0) (local.get 1))))
    WAT), [
    'env' => ['log' => fn (int $value) => print("wasm says $value\n")],
]);

var_dump($instance->exports->add(1, 2)); // wasm says 1, int(3)
```

The extension is experimental. The API can still change before a 1.0 release.

## Installation

The package is meant to be installed with [PIE](https://github.com/php/pie):

```sh
pie install veewee/ext-wasm
```

It is not on Packagist yet and has no release, so until then build it from a checkout as shown below.

The release workflow attaches prebuilt binaries for PHP 8.2 to 8.5 to each release, and PIE picks the one for your platform:

| Platform | Prebuilt | Without a prebuilt binary |
|---|---|---|
| Linux x86_64 and arm64, glibc 2.35 or newer | NTS and ZTS | |
| macOS arm64 | NTS | ZTS: PIE builds from source |
| Windows x86_64 | NTS | PIE cannot build from source on Windows, so ZTS PHP is not supported there |
| Linux with musl (Alpine), macOS x86_64, other Unix | | PIE builds from source, which needs a Rust toolchain from [rustup.rs](https://rustup.rs) and clang |

CI runs the test suite on Linux, macOS and Windows for PHP 8.2 to 8.5, and on thread-safe PHP 8.4 on Linux.

To build from a checkout:

```sh
cargo build --release
php -d extension=target/release/libwasm.so your-script.php   # libwasm.dylib on macOS, wasm.dll on Windows
```

On Windows this needs nightly Rust, because ext-php-rs uses the unstable vectorcall ABI there.

## Usage

### Modules and instances

`Wasm\Module` compiles a wasm binary or WAT text. Compiling is the expensive step, so compile once and instantiate as often as needed.

```php
$module = new Wasm\Module(file_get_contents('module.wasm'));
$module = Wasm\Module::fromFile('module.wasm');   // the same, without reading the file into a PHP string
$instance = new Wasm\Instance($module, $imports);

Wasm\validate($bytes);                            // bool
Wasm\compile($bytes);                             // Wasm\Module
Wasm\instantiate($bytes, $imports);               // ['module' => Module, 'instance' => Instance]
Wasm\instantiate($module, $imports);              // Instance

$module->exports();                               // [['name' => 'add', 'kind' => 'function'], ...]
$module->imports();                               // [['module' => 'env', 'name' => 'log', 'kind' => 'function'], ...]
$module->customSections('name');                  // list of binary strings
```

JS has these three as static functions on `WebAssembly.Module`. Here they are methods of the module.

`Module::fromFile()` reads local files only, relative to PHP's working directory and within `open_basedir`. Use `file_get_contents()` for stream wrappers such as `phar://`.

### Exports

`$instance->exports` holds every export by name. Functions are `Wasm\Func` objects, which are callable, and can also be called directly on the exports object. `Exports` is iterable in module order.

```php
$exports = $instance->exports;
$exports->add(1, 2);
$add = $exports->add;
array_map($add, [1, 2], [3, 4]);
```

A function without results returns `null`, one result is returned as is, and several results come back as a list.

### Imports

Imports use the shape of the JS import object: `['module' => ['name' => $value]]`. A value can be any PHP callable, or a `Func`, `Memory`, `Table`, `GlobalVar` or `Tag`. An immutable global import also accepts a plain number.

A PHP callback can call back into the same instance and read or write its memory while wasm is running, except in an async store (see [Async imports](#async-imports)). An exception thrown in a callback unwinds the wasm stack and reaches the caller as the original exception object.

### Memory, tables and globals

```php
$memory = new Wasm\Memory(['initial' => 1, 'maximum' => 10]);   // in 64 KiB pages
$memory->write(0, "hello");
$memory->read(0, 5);           // "hello"
$memory->grow(1);              // previous size in pages
$memory->byteLength();
$memory->buffer();             // a copy of the whole memory

$table = new Wasm\Table(['element' => 'anyfunc', 'initial' => 2], $exports->add);
$table->set(1, $exports->add);
$table->get(0);                // Wasm\Func
$table->grow(1);
$table->length();

$global = new Wasm\GlobalVar(['value' => 'i64', 'mutable' => true], 42);
$global->value = 43;
```

`Global` is a reserved word in PHP, which is why the class is called `GlobalVar`. PHP has no shared `ArrayBuffer`, so memory is read and written through copies instead of a live view.

### Stores

wasmtime keeps wasm objects in stores and frees memory one whole store at a time. The extension picks a store for every object you create:

- an instance joins the store of the `Memory`, `Table`, `GlobalVar`, `Tag` or `Func` objects it imports, and gets a store of its own when it imports none,
- a `Table` or `GlobalVar` joins the store of the function it starts with, as `$table` does above,
- any other `Memory`, `Table`, `GlobalVar` or `Tag` goes into a store that standalone objects share, so they can be imported together as in JS. Once an instance imports from that store, standalone objects created after it get a new shared store,
- the exports of an instance live in the store of that instance.

Dropping an instance together with its exports frees its memory, also in a long-running worker. Objects from two stores cannot be combined, so filling a standalone table with functions of an unrelated instance throws a `LinkError`. Group such objects in a `Wasm\Store`:

```php
$store = new Wasm\Store();
$math = new Wasm\Instance($mathModule, store: $store);
$table = new Wasm\Table(['element' => 'anyfunc', 'initial' => 1], store: $store);
$table->set(0, $math->exports->double);
```

`Memory`, `Table`, `GlobalVar`, `Tag` and `Instance` all accept `store:`. A store lives as long as any object in it or a `Wasm\Store` object for it. JS has no stores and lets any objects be combined.

### Memory limit

`wasm.memory_limit` caps the memory of each store: its linear memories, and its tables at 8 bytes per element, counted together. It takes a byte count with an optional `K`, `M` or `G` suffix (`64M`, `1G`), and `0` or `-1` means no limit, the default. `ini_set()` refuses an invalid value and returns false.

A store takes the value set when it is created and keeps it for its whole life. `ini_set()` therefore applies to stores created after it: a `Wasm\Store`, a `Wasm\Wasi`, a component instance, and a core instance that gets a store of its own. A core instance created with `store:`, or with imports from existing wasm objects (a standalone `Memory` or `Table`, another instance's exports, `$wasi->getImportObject()`), joins that store and gets that store's limit, whatever the setting is now.

```php
ini_set('wasm.memory_limit', '64M');
$instance = new Wasm\Instance($module);   // this instance and its memory get 64 MiB
ini_restore('wasm.memory_limit');
```

Growing past the limit fails the way wasm expects: `memory.grow` and `table.grow` return -1, and `Memory::grow()` and `Table::grow()` from PHP throw a `ValueError`. An instance whose initial memory or tables are already over the limit is a `LinkError`, and a standalone `Memory` or `Table` over it throws a `ValueError`. Those errors name `wasm.memory_limit`. Standalone objects created after the limit changed get a new shared store, so a limit set around one library call does not reach them.

There is no time limit for wasm code. PHP's `max_execution_time` does not interrupt a call that loops in wasm, but PHP's hard timeout ends the process `hard_timeout` seconds later (2 by default), without running shutdown functions. With `hard_timeout=0` the process does not end at all. This was observed with the CLI on PHP 8.4 (NTS) on macOS.

### WASI

`Wasm\Wasi` runs programs built for WASI, the system interface most wasm programs outside the browser use. Its shape follows Node's `WASI` class. A core module gets WASI preview1 through `getImportObject()`, and a component gets preview2 (see [Components](#components)):

```php
$wasi = new Wasm\Wasi(
    args: ['python', '-c', 'import sys; print(sys.stdin.read().upper())'],
    env: ['LANG' => 'C'],
    stdin: 'hello',
    preopens: ['/data' => '/srv/app/data'],
);
$instance = new Wasm\Instance($module, $wasi->getImportObject());
$exitCode = $wasi->start($instance);   // runs _start
$wasi->stdout();                       // "HELLO\n"
$wasi->stderr();
```

A module sees nothing of the host except what you pass: no environment variables, no stdio of the PHP process and no files outside the preopened directories. Preopens map a path inside the module to a host directory and are read-only unless you pass `['path' => '/srv/out', 'writable' => true]`.

stdout and stderr are captured and read after the run, up to `outputLimit` bytes each (16 MiB by default). A program that writes more gets an I/O error, and `start()` throws a `RuntimeError` afterwards. `start()` returns the exit code, also when the program calls `exit()`.

Modules that work as a library export `_initialize` instead of `_start`. Call `$wasi->initialize($instance)` once and then use the exports as usual. Such an export calling `exit()` throws a `RuntimeError`.

`getImportObject()` returns the WASI functions under `wasi_snapshot_preview1`, so you can combine them with imports of your own: `[...$wasi->getImportObject(), 'env' => [...]]`. A `Wasi` object belongs to one run of one module; create a new one for the next run.

### Components

A WebAssembly component describes its imports and exports in WIT, the interface language of the component model, so its functions take and return strings, records, lists and other typed values instead of numbers and memory addresses. The classes live in `Wasm\Component`, next to the core `Wasm\Module` and `Wasm\Instance`, and the value mapping is modelled on that of [jco](https://github.com/bytecodealliance/jco), the JavaScript toolchain for components, adapted where PHP differs:

```php
use Wasm\Component\Component;
use Wasm\Component\Instance;

$component = Component::fromFile('markdown.wasm');     // or new Component($bytesOrWat)
$component->exports();   // [['name' => 'docs:markdown/render@0.1.0', 'kind' => 'instance', 'functions' => [...]]]

$instance = new Instance($component, [
    'docs:demo/log' => ['writeLine' => fn (string $line) => error_log($line)],   // an imported interface
    'now' => fn (): int => time(),                                              // a function the world imports
], wasi: new Wasm\Wasi());

$instance->exports->get('docs:markdown/render')->render('**hi**');   // an exported interface
$instance->exports->renderHtml('**hi**');                            // a function the world exports
```

`exports()` and `imports()` list every function with its WIT signature as text, such as `func(markdown: string) -> string`, and as a `Wasm\Component\Type\FunctionType` under `signature`, which `Func::type()` returns too. Its `params` and `result` are `Wasm\Component\Type\ValueType` objects: `kind` is the WIT keyword (`u32`, `record`, `own`, ...), and `element`, `types`, `fields`, `cases`, `names`, `ok`, `err` and `resource` describe what the kind holds. A type carries the `name` the component gives it, whatever its kind. Types are matched by shape, so an unnamed type with the same shape as a named one gets that name too. The name is left out when two names fit the same shape or the type holds a resource. This can be used, for example, to generate PHP stubs for a component. `get()` takes an export by its WIT name, with or without the version, and function names become camelCase methods, so `render-html` is `renderHtml()`. Imports are keyed the same way: an interface by its name, with or without version, holding its functions by camelCase name.

| WIT | PHP |
|---|---|
| `bool`, `string`, `char` | `bool`, `string`, a string of one character |
| `s8` to `s64`, `u8` to `u32` | `int`, range-checked |
| `u64` | `int`; values above `PHP_INT_MAX` keep their bits and come out negative, as with `unpack('J')` |
| `f32`, `f64` | `float`, also accepting `int` |
| `list<u8>` | binary string; every byte crosses as a value of its own, and about 4 MB exceeds wasmtime's copy limit for one call |
| `list<T>`, `tuple<...>` | list array |
| `record` | array with camelCase keys; `option` fields may be left out |
| `flags` | array of camelCase names to `bool` |
| `enum` | its case name as a string, such as `'dark-blue'` |
| `option<T>` | `null` or the value |
| `variant` | `new Wasm\Component\Variant('case-name', $payload)` |
| `result` returned by a function | the ok value, or a thrown `Wasm\Exception\ComponentError` with `$payload` |
| `result` anywhere else | `Wasm\Component\Result::ok($value)` or `Result::err($error)` |

A PHP import returns its ok value directly and signals an err by throwing `new ComponentError($payload)`, or it returns a `Result`. Any other exception reaches the caller as the original object. Unlike a core instance, a component instance is unusable after a call fails, whether through a trap or an exception from an import: the component model marks it as trapped, and the next call throws a `RuntimeError` "cannot enter component instance". A PHP import may call back into its own instance.

With `wasi:`, the `Wasm\Wasi` object provides every `wasi:*` import, as preview2, with the same sandbox as for core modules. `$wasi->start($instance)` runs a command component's `wasi:cli/run` and returns its exit code. Rust's standard library on `wasm32-wasip2` reports any failing exit as 1. A component that imports WASI without a `Wasi` object fails with a `LinkError`, like any missing import.

A component can make HTTP and HTTPS requests through `wasi:http` when the `Wasi` object lists the hosts it may reach:

```php
$wasi = new Wasm\Wasi(httpHosts: ['api.example.com', 'localhost:8080', '*.example.org']);
```

An entry is a host (any port), `host:port` (only that port; a URL without a port uses 80 or 443), or `*.domain` (its subdomains, not the domain itself). Hosts are compared without case, IPv6 addresses are written in brackets (`[::1]:8080`), and international domains in punycode. A request to any other host fails inside the component with `HttpRequestDenied` before anything is sent. Without `httpHosts`, a component that imports `wasi:http` fails with a `LinkError`, and an empty list denies every request. Connecting, waiting for the response headers and every wait between body chunks are each limited to PHP's `default_socket_timeout`, read when the component is instantiated, and the whole setup until the headers, TLS handshake included, to twice that. With a timeout of 0 or less, wasmtime's own limit of 600 seconds per step applies and the TLS handshake has none. Redirects are not followed, so the component sees them and every next request is checked again. The list is checked by name: an allowed name that resolves to a private address still connects. HTTPS uses rustls with the Mozilla root certificates built in. `httpHosts` has no effect for core modules, which have no HTTP in WASI preview1.

A component that exports `wasi:http/incoming-handler`, such as one built for `wasi:http/proxy`, can answer HTTP requests from PHP:

```php
use Wasm\Component\Http\Request;

$instance = new Instance($component, wasi: new Wasm\Wasi(httpHosts: []));
$response = $instance->handle(new Request('POST', 'https://example.com/api?x=1', ['content-type' => 'application/json'], $json));
$response->status;    // 200
$response->headers;   // ['content-type' => ['application/json']], names lowercase, values as lists
$response->body;      // the whole body as a string
```

`Request` and `Response` are small read-only value objects of the extension, so no PSR-7 package is needed; converting from and to one takes a few lines in userland. The proxy world imports the HTTP types, so the `Wasi` object needs `httpHosts`, and an empty list is enough when the component makes no requests itself. Headers that HTTP handles by itself, such as `host`, `connection` and `transfer-encoding`, are left out of the request the component sees; the host is part of its URL. An error code the component answers with, a component that never sets a response, and a trap are each a `RuntimeError`.

Resources, the WIT types with handles and methods, work in both directions. A resource a component exports is a `Wasm\Component\ResourceClass` in its interface, and its handles are `Wasm\Component\Resource` objects:

```php
$counters = $instance->exports->get('docs:demo/counters');
$counter = $counters->get('counter')->new(5);   // [constructor]counter
$counter->increment();                          // [method]counter.increment
$counters->get('counter')->zero();              // [static]counter.zero
$counter->drop();                               // or let PHP release it
```

Releasing the PHP object drops the handle, and the component runs its destructor for it. Passing a handle where WIT expects an owned value moves it into the component: the PHP object is unusable afterwards. A borrowed parameter leaves it with PHP. A method called `drop` is reached with `$counter->call('drop')`.

A resource a component imports is implemented by a PHP class, given by name in the import object:

```php
$instance = new Instance($component, ['docs:demo/log' => ['logger' => MyLogger::class]], wasi: $wasi);
```

`[constructor]logger` runs `new MyLogger(...)`, `[method]logger.write` calls `$logger->write(...)` and `[static]logger.from-env` calls `MyLogger::fromEnv(...)`. The class must have each method and static function the resource declares, or instantiating is a `LinkError`. A PHP object passed to the component comes back as the same object, and the component dropping its handle releases the object.

One instance's exports can be another's imports. An interface one instance exports can be given as the value of an interface import that declares the same functions, and an exported `ResourceClass` can implement a resource import inside an interface array. Each instance needs a `Wasi` object of its own:

```php
$counters = (new Instance(Component::fromFile('counters.wasm'), wasi: new Wasi()))->exports->get('docs:demo/counters');
$composer = new Instance(Component::fromFile('composer.wasm'), ['docs:demo/counters' => $counters], new Wasi());
// or: ['docs:demo/counters' => ['counter' => $counters->get('counter'), 'total' => fn (Resource $a, Resource $b): int => $a->value() + $b->value()]]
```

wasmtime's component linker cannot define an import from another instance's export, so every call between them goes through PHP and converts its values on the way. The importing instance works with the exporting instance's `Resource` objects: a handle it returns is the same PHP object that went in, and one it drops is released to PHP, which drops it in the exporting instance when the last reference goes. A handle of another instance, or one that was dropped or moved, is refused before the call. Linking compares names, not signatures, so a function whose parameters differ between the two fails at its first call, and from then on every call into the importing instance throws a `RuntimeError`. A `Suspending` import of the exporting instance can suspend its Fiber only when the importing instance reached it through a `Suspending` import of its own; through a plain import it throws a `FiberError`. An instance needs its imports when it is created, so two instances cannot import each other's exports. PHP imports that hold on to each other's instances form a cycle the garbage collector does not free, and those instances stay alive until the end of the request.

A component import may be a `Wasm\Suspending` too, at the world level or inside an imported interface, and then suspends its Fiber as core imports do (see [Async imports](#async-imports)). Every other PHP import of that instance still blocks Fiber switches, and while a call waits, calling into the same instance throws a `RuntimeError` "the store is busy with a suspended call". Resource constructors and methods implemented by PHP classes cannot suspend. A component's resource destructor runs where PHP releases the handle and cannot suspend either: a `Suspending` import it calls may return, but one that suspends throws a `FiberError` there and, like any failed call, leaves the instance unusable.

Components cannot be combined with core objects: a component instance has a store of its own. A component that uses `map` or fixed-length lists fails to compile with a `CompileError`.

[examples/rust-markdown](examples/rust-markdown) is a Rust component built with wit-bindgen.

### Async imports

A PHP callback wrapped in `Wasm\Suspending` may suspend its Fiber while wasm waits for it, like `WebAssembly.Suspending` with JS Promise Integration. Other Fibers keep running meanwhile, so an event loop such as [Amp](https://amphp.org) can wait on many wasm calls at once:

```php
$imports = ['host' => [
    'lookup' => new Wasm\Suspending(function (int $id): int {
        Amp\delay(0.1);              // suspends this Fiber only
        return $id * 2;
    }),
]];

$futures = array_map(
    fn (int $id) => Amp\async(fn () => (new Wasm\Instance($module, $imports))->exports->run($id)),
    range(1, 10),
);
$results = Amp\Future\await($futures);   // about 0.1 s in total
```

In JS the export also has to be wrapped in `WebAssembly.promising()`. PHP needs no wrapper, because calling the export only blocks the Fiber that called it.

An instance with a `Suspending` import makes its store async. In an async store every PHP callback, Suspending or plain, runs while its wasm call is paused, and wasmtime's garbage collector cannot see the frames of a paused call. So while a callback runs, the store throws a `RuntimeError` "the store is busy with a suspended call" for anything that could start the collector: calling its exports, instantiating into it, `Wasi::start()` or `initialize()`, and passing a new PHP value as an externref. This applies inside the callback and in other Fibers alike. Memory, globals and tables stay usable.

In practice, give every Fiber an instance of its own, and have the callback write its answer into memory at an address wasm passes in instead of calling an allocator export. A store that already has plain PHP callbacks cannot take Suspending imports and throws a `LinkError`. Plain callbacks in an async store still cannot switch Fibers, and a call into an async store takes about 0.1 microseconds longer than into a sync one, measured on an Apple Silicon Mac.

[examples/async](examples/async) runs ten lookups concurrently through Amp.

### Values

| Wasm type | From PHP | To PHP |
|---|---|---|
| `i32` | `int` from -2^31 to 2^32-1 | signed `int` |
| `i64` | `int` | `int` |
| `f32`, `f64` | `int` or `float` | `float` |
| `v128` | 16 byte string | 16 byte string |
| `externref` | any PHP value, `null` for a null reference | the same value, by identity |
| `funcref` | `Wasm\Func` or `null` | `Wasm\Func` or `null` |

Conversion is strict where JS coerces: passing `'1'` or `1.5` for an `i32` throws a `TypeError` and an out of range integer throws a `ValueError`.

### Errors

Everything the engine raises extends `Wasm\Exception\WasmException`:

- `CompileError` for invalid wasm or WAT,
- `LinkError` for missing or mismatched imports, and for combining objects of two stores,
- `RuntimeError` for traps such as `unreachable`, out of bounds access or stack exhaustion, with the wasm backtrace in the message,
- `WasmThrow` for a wasm exception (the exception handling proposal) that reaches PHP. It carries `$tag` and `$payload`. A PHP callback can throw `new WasmThrow($tag, $payload)` for wasm code to catch.

## Examples

The [examples](examples) folder has small scripts for each feature, and nine larger ones:

- [examples/doom](examples/doom) plays DOOM in your terminal, with PHP running the game loop, the keyboard and the drawing.
- [examples/mago](examples/mago) runs the formatter of [mago](https://github.com/carthage-software/mago) from its official wasm build.
- [examples/html](examples/html) sanitizes untrusted HTML with ammonia and rewrites responses with Cloudflare's lol-html, for lazy images, CSP nonces and safe links.
- [examples/python](examples/python) runs Python code in CPython 3.12 compiled to WASI.
- [examples/quickjs](examples/quickjs) shares JavaScript checkout rules between the browser and PHP, running them in QuickJS through WASI.
- [examples/oxipng](examples/oxipng) optimises PNG files losslessly with oxipng, taken from an npm package built for browsers.
- [examples/rust-markdown](examples/rust-markdown) writes part of a PHP application in Rust: a Markdown renderer built on pulldown-cmark as a component, called with PHP strings.
- [examples/async](examples/async) runs ten wasm lookups concurrently with Amp through `Wasm\Suspending` imports.
- [examples/typst](examples/typst) renders PDF invoices from a Typst template and PHP data, with the Typst compiler built to wasm.

## Compilation cache

Compiling a large module to machine code takes a while: mago's 18 MB build needs about three seconds. Like a browser, the extension keeps compiled code in a cache on disk, keyed by the module bytes and the engine settings, so the next process loads it in milliseconds. A changed module or a new extension version simply compiles again.

| Setting | Default | Meaning |
|---|---|---|
| `wasm.cache` | `1` | Enables the cache. |
| `wasm.cache_dir` | empty | Directory for cached code. Empty uses wasmtime's default, `~/.cache/wasmtime` on Linux and `~/Library/Caches/BytecodeAlliance.wasmtime` on macOS. |

Both can only be set in php.ini or with `-d`, because the engine is created once per process. If the directory cannot be created or written, the extension compiles without the cache.

The cache holds machine code that runs inside the PHP process, so anyone who can write to that directory can run code as the PHP user. On a server, point `wasm.cache_dir` at a directory only the PHP user can write to.

## Limits worth knowing

- Recursion that alternates between wasm and PHP callbacks counts against wasmtime's 512 KiB stack budget, which allows roughly 140 levels in a release build. Going deeper throws a `RuntimeError` rather than crashing.
- wasmtime frees an instance only together with its store (see [Stores](#stores)). In a long-running worker (RoadRunner, FrankenPHP worker mode, Swoole), cache the `Module` between requests, which is not tied to a store. A standalone object you keep for the whole worker, such as a cached `Memory`, keeps its store alive, and with it every instance that imports it. Give such objects their own `Wasm\Store`, or create them per job.
- PHP values held by wasm (externref, callables behind imports) are invisible to PHP's cycle collector. A callback that captures its own instance, or an object the instance imports, keeps that instance and its store alive until the PHP process ends.
- A plain PHP callback cannot switch Fibers while wasm waits for it: `Fiber::suspend()` inside it throws a `FiberError`. Wrap the callback in `Wasm\Suspending` to allow it (see [Async imports](#async-imports)). Calling wasm from inside a Fiber, and suspending between calls, works as usual.
- WASI covers preview1 for core modules and preview2 for components, not the async preview3. File access in a forked child after the parent used WASI is tested for both. A program that waits on a file and a timer at once in a forked child has not been tested.

## Development

```sh
make debug          # cargo build
make compile        # cargo build --release
make phpunit        # run the tests against the release build
make stubs          # regenerate ext-wasm.stubs.php (needs cargo install cargo-php)
```

## License

MIT
