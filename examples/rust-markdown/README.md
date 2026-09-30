# Rust code in PHP

This example shows how to write part of a PHP application in Rust. `src/lib.rs` is a few lines of Rust around [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark), a fast CommonMark parser, built as a WebAssembly component. `Markdown.php` loads it and turns Markdown into HTML with one call that takes and returns a PHP string.

## Running it

Install the extension as described in the [main README](../../README.md). The compiled `markdown.wasm` is checked in, so running it needs no Rust:

```sh
php examples/rust-markdown/render.php
php examples/rust-markdown/render.php < README.md
```

## Changing the Rust code

Install Rust from [rustup.rs](https://rustup.rs), edit `src/lib.rs` and rebuild the component:

```sh
examples/rust-markdown/build.sh
```

The script adds the `wasm32-wasip2` target once, builds with size optimisations and copies the result next to `Markdown.php`. That target produces a component directly.

## String transfer between PHP and Rust

`wit/markdown.wit` describes the interface in WIT, the interface language of the component model:

```wit
interface render {
    render: func(markdown: string) -> string;
}
```

[wit-bindgen](https://github.com/bytecodealliance/wit-bindgen) generates the Rust side from it, and the extension converts PHP strings on the other side, so neither the Rust code nor `Markdown.php` handles memory or pointers. The same works for records, lists, options and results; the main README lists how each WIT type maps to PHP.

A WIT `string` is UTF-8, so `toHtml()` throws a `ValueError` for input in another encoding; convert it first with `mb_convert_encoding()`.

Rust's standard library imports a few WASI interfaces, such as a random seed for its hash maps, so `Markdown.php` passes a `Wasm\Wasi` object. It gives the component no files, environment or arguments.
