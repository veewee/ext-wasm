# Lossless PNG optimisation

This example shrinks PNG files without changing a pixel, using [oxipng](https://github.com/shssoichiro/oxipng) compiled to wasm. The module comes from the [@jsquash/oxipng](https://www.npmjs.com/package/@jsquash/oxipng) npm package, which is built for browsers with wasm-bindgen. PHP uses the same 164 KB file without any JavaScript.

## Running it

Install the extension as described in the [main README](../../README.md), then download the module and optimise a PNG:

```sh
examples/oxipng/download.sh
php examples/oxipng/optimise.php                      # a generated sample
php examples/oxipng/optimise.php photo.png smaller.png
```

On the generated sample, a gradient stored without compression, it prints `262488 bytes -> 758 bytes`.

## Using a wasm-bindgen module from PHP

`Oxipng.php` does in PHP what wasm-bindgen's JavaScript glue does in the browser. Reading `codec/pkg/squoosh_oxipng.js` in the npm package shows the steps:

1. Reserve 16 bytes on wasm's own stack with `__wbindgen_add_to_stack_pointer(-16)`. The export writes its result's address and length there.
2. Copy the PNG into the module's memory with `__wbindgen_malloc()` and `$memory->write()`.
3. Call `optimise()`, read the result from the return slot and free it with `__wbindgen_free()`.
4. Restore the stack pointer, also when the call failed.

The one import, `wbg.__wbindgen_throw`, receives an error message as an address and a length. The PHP callback reads it from memory and throws it as an exception. It reaches the exports through a `WeakReference`, because a callback that holds its own instance keeps that instance alive until the process ends (see the limits in the main README).

This works for wasm-bindgen modules whose imports are only `__wbindgen_throw` and similar plain functions. Modules that pass JavaScript objects around need wasm-bindgen's object table, which is a lot more work to rebuild in PHP.
