# Concurrent lookups from wasm with Amp

This example runs ten wasm instances side by side, one per Fiber. Each one greets a person, and to get the name it calls a `lookup` import that PHP answers after 0.1 s. The import is a `Wasm\Suspending`, so the callback can wait with `Amp\delay()`, which stands in for an HTTP call or a database query. That suspends only its own Fiber, so the ten lookups wait together and the run takes about 0.1 s instead of a second. A ticker on the Revolt event loop keeps running the whole time.

The callback writes the name straight into the instance's memory and returns its length. It can't ask the module to allocate a buffer, because a store rejects calls into its wasm while one of its calls is suspended. So the module hands over a pointer and a capacity instead.

## Running it

Install the extension as described in the [main README](../../README.md), then install Amp and run it:

```sh
composer install -d examples/async
php examples/async/lookup.php
```

It prints the ten greetings, how long the lookups took, and "concurrent" when they overlapped.
