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

Install it with [PIE](https://github.com/php/pie):

```sh
pie install veewee/ext-wasm
```

PIE downloads a prebuilt binary for Linux (x86_64 and arm64, glibc) and macOS (arm64) on PHP 8.2 to 8.5. On other platforms it builds from source, which needs a Rust toolchain from [rustup.rs](https://rustup.rs) and clang.

To build from a checkout instead:

```sh
cargo build --release
php -d extension=target/release/libwasm.so your-script.php   # libwasm.dylib on macOS
```

Windows is not supported by PIE yet. The extension builds there with nightly Rust, which ext-php-rs requires on Windows.

## Usage

### Modules and instances

`Wasm\Module` compiles a wasm binary or WAT text. Compiling is the expensive step, so compile once and instantiate as often as needed.

```php
$module = new Wasm\Module(file_get_contents('module.wasm'));
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

A PHP callback can call back into the same instance and read or write its memory while wasm is running. An exception thrown in a callback unwinds the wasm stack and reaches the caller as the original exception object.

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

### WASI

`Wasm\Wasi` runs modules built for WASI preview1, the system interface most wasm programs outside the browser use. Its shape follows Node's `WASI` class:

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

The [examples](examples) folder has small scripts for each feature, and four larger ones:

- [examples/doom](examples/doom) plays DOOM in your terminal, with PHP running the game loop, the keyboard and the drawing.
- [examples/mago](examples/mago) runs the formatter of [mago](https://github.com/carthage-software/mago) from its official wasm build.
- [examples/python](examples/python) runs Python code in CPython 3.12 compiled to WASI.
- [examples/quickjs](examples/quickjs) shares JavaScript checkout rules between the browser and PHP, running them in QuickJS through WASI.

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
- A PHP callback cannot switch fibers while wasm waits for it: `Fiber::suspend()` inside a callback throws a `FiberError`. Calling wasm from inside a fiber, and suspending between calls, works as usual.
- WASI support covers preview1, not preview2 and the component model. A WASI program that waits on a file and a timer at once in a forked child has not been tested and might hang.

## Development

```sh
make debug          # cargo build
make compile        # cargo build --release
make phpunit        # run the tests against the release build
make stubs          # regenerate ext-wasm.stubs.php (needs cargo install cargo-php)
```

## License

MIT
