# Rust code in PHP

This example shows how to write part of a PHP application in Rust. `src/lib.rs` is a few lines of Rust around [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark), a fast CommonMark parser, compiled to a 200 KB wasm module with no imports. `Markdown.php` loads it and turns Markdown into HTML.

## Running it

Install the extension as described in the [main README](../../README.md). The compiled `markdown.wasm` is checked in, so running it needs no Rust:

```sh
php examples/rust-markdown/render.php
php examples/rust-markdown/render.php < README.md
```

## Changing the Rust code

Install Rust from [rustup.rs](https://rustup.rs), edit `src/lib.rs` and rebuild the module:

```sh
examples/rust-markdown/build.sh
```

The script adds the `wasm32-unknown-unknown` target once, builds with size optimisations and copies the result next to `Markdown.php`.

## Passing strings between PHP and Rust

wasm functions only take and return numbers, so strings travel through the module's memory:

1. PHP calls `alloc(length)`, which reserves room in the module's memory and returns its address.
2. PHP writes the Markdown there with `$memory->write()` and calls `render(address, length)`.
3. Rust renders into a new buffer and returns its address and length packed into one 64-bit integer.
4. PHP reads the HTML with `$memory->read()` and frees both buffers with `dealloc()`.

The same pattern works for any Rust function that takes and returns strings or bytes, for example JSON in and JSON out. A 90 KB document renders in about 3 ms on an Apple M-series machine.
