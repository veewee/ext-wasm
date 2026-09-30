# PDF invoices with Typst

This example renders PDFs from PHP with [Typst](https://typst.app), a modern typesetting system, without wkhtmltopdf, a headless browser or a Typst binary on the server. `src/lib.rs` wraps the Typst compiler and its PDF exporter in a wasm module with no imports, and embeds the fonts that ship with Typst. `Typst.php` loads it, and `invoice.php` renders `invoice.typ` with invoice data from a PHP array.

## Running it

Install the extension as described in the [main README](../../README.md) and Rust from [rustup.rs](https://rustup.rs). The module is 31 MB, so it is not checked in. Build it once, which takes a few minutes the first time, then render the invoice:

```sh
examples/typst/build.sh
php examples/typst/invoice.php invoice.pdf
```

Compiling the module to machine code happens once and then comes from the [compilation cache](../../README.md#compilation-cache). After that, the invoice renders in about 15 ms on an Apple M-series machine.

## Templates and data

The template is plain Typst. PHP passes the data as JSON, and the template reads it as a file:

```typst
#let invoice = json("data.json")
= Invoice #invoice.number
```

```php
$pdf = (new Typst())->pdf($template, ['number' => '2026-0042']);
```

The module has no file system, so `data.json` is the only file a template can read. Images and extra fonts would need more virtual files in `src/lib.rs`, the same way `data.json` is served. The fonts are the ones in [typst-assets](https://github.com/typst/typst-assets): Libertinus Serif, New Computer Modern and DejaVu Sans Mono. `datetime.today()` returns `none`, so pass dates in the data.

A template that does not compile throws a `RuntimeException` with Typst's messages and the line they point to, for example `main.typ:3: unknown variable: nope`.
