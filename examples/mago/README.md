# Mago's formatter from PHP

[Mago](https://github.com/carthage-software/mago) is a PHP linter, formatter and static analyzer written in Rust. Its releases include a WebAssembly build for the browser playground. This example runs mago's formatter from that official build, straight from PHP, without Node or a mago binary.

## Running it

Install the extension (see the main README) and download the wasm build into `examples/mago/dist`:

```sh
examples/mago/download.sh          # mago 1.50.0
examples/mago/download.sh 1.51.0   # or another release
```

Then format a file:

```sh
php examples/mago/format.php path/to/file.php
```

Given this input:

```php
<?php
function  greet( string $name ){
return "Hello ".$name ;}
echo greet( 42 ) ;
```

it prints:

```php
<?php

function greet(string $name)
{
    return 'Hello ' . $name;
}

echo greet(42);
```

Compiling the 18 MB module takes about three seconds; formatting itself takes a few milliseconds.

## Inner workings

The official build targets JavaScript: it was made with [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen), which pairs the wasm file with generated JS glue. The wasm imports a set of JS helper functions from that glue, mostly to build JS arrays and objects.

Formatting does not need any of them. A string goes in and a string comes out, both as a pointer and length in the module's memory. So `format.php` gives every import a stub that throws, copies the code into wasm memory with the module's exported allocator, calls `format` and reads the result back, which is exactly what the JS glue does.

The analyzer (`analyze`) is also exported, but it returns a JS array of JS objects. Those only exist inside the JS glue, so using it from PHP would mean reimplementing wasm-bindgen's JS object model. That becomes a single call like `format` as soon as mago exports the result as a JSON string.
