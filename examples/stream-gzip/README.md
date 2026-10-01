# Streaming gzip

This example compresses data of any size with an async WebAssembly component that reads and writes streams. `wit/gzip.wit` declares one function:

```wit
compress: async func(input: stream<u8>, level: u32) -> stream<u8>;
```

PHP passes a generator that reads a file in chunks, and gets a `Wasm\Component\Stream` of compressed chunks back. The component asks for the next input chunk only when it needs one, and writes output only while PHP reads, so PHP holds a chunk or two at a time instead of the whole file. `src/lib.rs` is [flate2](https://github.com/rust-lang/flate2-rs) with its pure Rust backend, around forty lines.

## Running it

Install the extension as described in the [main README](../../README.md). The compiled `stream-gzip.wasm` is checked in, so running it needs no Rust.

Compress a file, or 50 MB of generated log lines without an argument:

```sh
php examples/stream-gzip/compress.php
php examples/stream-gzip/compress.php access.log access.log.gz
```

It prints the sizes, the time and PHP's peak memory. On an M-series Mac with a release build, the generated 56.7 MB compress to 3.3 MB in about 0.6 seconds with a peak of 0.7 MB. PHP's own `gzencode()` is faster on data that fits in memory, because zlib is native code; what the component shows is a streaming pipeline in a few lines of PHP.

Serve files compressed while they are compressed, flushing every chunk as it comes:

```sh
php -S localhost:8000 examples/stream-gzip/server.php
curl --compressed localhost:8000/README.md
```

## From PHP

```php
$gzip = new Gzip();
foreach ($gzip->compress(Gzip::read('big.log'), level: 9) as $chunk) {
    fwrite($out, $chunk);
}
```

`compress()` takes any iterable of byte strings: an array, a generator, an `IteratorAggregate`. Every element is one round trip between the component and PHP, so chunks of some kilobytes keep that overhead small.

## Changing the Rust code

Install Rust from [rustup.rs](https://rustup.rs), edit `src/lib.rs` and rebuild:

```sh
examples/stream-gzip/build.sh
```

The component is built with wit-bindgen's async support for the `wasm32-wasip2` target; no WASI 0.3 is involved.
