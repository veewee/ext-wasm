# Mago from PHP

[Mago](https://github.com/carthage-software/mago) is a PHP linter, formatter and static analyzer written in Rust. Its releases include a WebAssembly build meant for the browser playground. This example runs that same build from PHP with ext-wasm, without Node or a mago binary.

## Running it

Install the extension first (see the main README), then download the wasm build. The script fetches the official release asset into `examples/mago/dist`:

```sh
examples/mago/download.sh          # defaults to mago 1.50.0
examples/mago/download.sh 1.50.0   # or pick a version
```

Format a file:

```sh
php examples/mago/mago.php format path/to/file.php
```

Analyze a file:

```sh
php examples/mago/mago.php analyze path/to/file.php
```

Given this input:

```php
<?php
function  greet( string $name ){
return "Hello ".$name ;}
echo greet( 42 ) ;
echo $undefinedVariable;
```

`format` prints:

```php
<?php

function greet(string $name)
{
    return 'Hello ' . $name;
}

echo greet(42);
echo $undefinedVariable;
```

and `analyze` reports:

```text
WARNING [missing-return-type] Function `greet` is missing a return type hint.
ERROR [invalid-argument] Invalid argument type for argument #1 of `greet`: expected `string`, but found `int(42)`.
ERROR [mixed-argument] The first value for `echo` is too general.
ERROR [undefined-variable] Undefined variable: `$undefinedVariable`.
ERROR [mixed-argument] The first value for `echo` is too general.
5 issue(s)
```

Compiling the 18 MB module takes about three seconds, after which each call runs in milliseconds.

## Inner workings

The build is made with wasm-bindgen for JavaScript, so it expects a JS host: it imports 37 functions from `mago_wasm_bg.js` and keeps every JS value it creates in a heap on the host side. `WasmBindgenHost.php` implements that heap in PHP, together with the handful of imports `format` and `analyze` use to build strings, arrays and objects. Arguments go in the way the JS glue passes them: allocate with the module's exported allocator, copy the bytes into memory and pass pointer and length.

Import names carry a hash that changes with every build, and a few imports share both name and signature, so the host reads the generated `mago_wasm_bg.js` next to the wasm file and decides from each function body what an import does. Imports it does not implement throw with their name, so a newer mago build that needs more is easy to spot.
