# JavaScript rules shared by the browser and PHP

This example runs the same JavaScript file in the browser and in PHP. `js/rules.js` holds checkout rules: which countries you ship to, VAT in integer cents, and order validation. The browser imports it directly, and PHP runs it in [QuickJS-ng](https://github.com/quickjs-ng/quickjs), a JavaScript engine compiled to WASI. So both sides validate and round the same way, and there's no second implementation that drifts.

The JavaScript runs sandboxed. It sees the `js` directory read-only, gets the order on stdin and answers on stdout. It can't reach the network, the environment or any other file.

## Running it

Install the extension as described in the [main README](../../README.md), then download the engine (1.5 MB) and check two orders:

```sh
examples/quickjs/download.sh
php examples/quickjs/checkout.php
```

To see the same rules in the browser, serve the directory and open http://localhost:8000:

```sh
php -S localhost:8000 -t examples/quickjs
```

## How it fits together

- `js/rules.js` exports `totals()` and `validate()`, plain JavaScript without dependencies.
- `js/main.js` is the entry point for QuickJS: it reads the order from stdin, applies the rules and prints JSON.
- `checkout.php` starts QuickJS per order through `Wasm\Wasi`, with the `js` directory preopened as `/app`.
- `index.html` imports `js/rules.js` as an ES module and shows the result as you edit the order.

Every call starts a fresh interpreter. Once the module is compiled, one run of these rules took about 0.4 ms on an Apple M-series machine.
