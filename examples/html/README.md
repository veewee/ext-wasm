# HTML sanitizing and rewriting

This example cleans untrusted HTML and rewrites HTML responses with two Rust crates compiled into one 1.1 MB wasm module with no imports:

- [ammonia](https://github.com/rust-ammonia/ammonia) sanitizes HTML against an allow list, so a user comment keeps its `<b>` and loses its `<script>`, its `onclick` and its `javascript:` links.
- [lol-html](https://github.com/cloudflare/lol-html) is the streaming HTML rewriter behind Cloudflare Workers' `HTMLRewriter`. It changes elements matched by CSS selectors without building a DOM.

`Html.php` loads the module. `middleware.php` does what a response middleware would: it sanitizes a comment, adds `loading="lazy"` to images, puts a CSP nonce on every script, marks external links `rel="noopener"` and removes ads.

## Running it

Install the extension as described in the [main README](../../README.md). The compiled `html.wasm` is checked in, so running it needs no Rust:

```sh
php examples/html/middleware.php            # a built-in sample page
php examples/html/middleware.php page.html
```

## Rules

The rewrite rules come from PHP as a list, and the module applies them in one pass over the document:

```php
$html->rewrite($page, [
    ['selector' => 'img', 'set' => ['loading' => 'lazy']],
    ['selector' => 'a[href^="http"]', 'set' => ['rel' => 'noopener']],
    ['selector' => '.ad', 'remove' => true],
]);
```

`selector` takes the CSS selectors lol-html supports, `set` sets attributes and replaces any existing value, and `remove` drops the element with its content. The nonce rule in `middleware.php` puts a valid nonce on every script it finds, so run untrusted markup through `sanitize()` before it lands in the page. An invalid selector throws an `InvalidArgumentException` with lol-html's message.

## Changing the Rust code

Install Rust from [rustup.rs](https://rustup.rs), edit `src/lib.rs` and rebuild the module:

```sh
examples/html/build.sh
```

Strings pass between PHP and Rust through the module's memory, as the [rust-markdown example](../rust-markdown) explains. `rewrite()` can fail, so its output starts with one status byte: 0 when the rest is HTML, 1 when it is an error message.
