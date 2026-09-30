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

Wasm\Module::exports($module);                    // [['name' => 'add', 'kind' => 'function'], ...]
Wasm\Module::imports($module);                    // [['module' => 'env', 'name' => 'log', 'kind' => 'function'], ...]
Wasm\Module::customSections($module, 'name');     // list of binary strings
```

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

$table = new Wasm\Table(['element' => 'anyfunc', 'initial' => 2]);
$table->set(0, $exports->add);
$table->get(0);                // Wasm\Func
$table->grow(1);
$table->length();

$global = new Wasm\GlobalVar(['value' => 'i64', 'mutable' => true], 42);
$global->value = 43;
```

`Global` is a reserved word in PHP, which is why the class is called `GlobalVar`. PHP has no shared `ArrayBuffer`, so memory is read and written through copies instead of a live view.

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
- `LinkError` for missing or mismatched imports,
- `RuntimeError` for traps such as `unreachable`, out of bounds access or stack exhaustion, with the wasm backtrace in the message,
- `WasmThrow` for a wasm exception (the exception handling proposal) that reaches PHP. It carries `$tag` and `$payload`. A PHP callback can throw `new WasmThrow($tag, $payload)` for wasm code to catch.

## Examples

The [examples](examples) folder has small scripts for each feature. [examples/mago](examples/mago) runs the formatter of [mago](https://github.com/carthage-software/mago) from its official wasm build.

## Limits worth knowing

- Recursion that alternates between wasm and PHP callbacks counts against wasmtime's 512 KiB stack budget, which allows roughly 140 levels in a release build. Going deeper throws a `RuntimeError` rather than crashing.
- PHP values held by wasm (externref, callables behind imports) are invisible to PHP's cycle collector. A callback that captures its own instance keeps that instance alive until the PHP process ends.
- A PHP callback cannot switch fibers while wasm waits for it: `Fiber::suspend()` inside a callback throws a `FiberError`. Calling wasm from inside a fiber, and suspending between calls, works as usual.
- WASI is not supported yet.

## Development

```sh
make debug          # cargo build
make compile        # cargo build --release
make phpunit        # run the tests against the release build
make stubs          # regenerate ext-wasm.stubs.php (needs cargo install cargo-php)
```

## License

MIT
