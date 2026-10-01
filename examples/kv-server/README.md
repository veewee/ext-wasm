# Key-value server

This example is a small Redis compatible key-value server, where PHP owns the sockets and a component speaks the protocol. `server.php` listens with `stream_socket_server`, accepts a connection and hands it to one async export, declared in `wit/kv.wit`:

```wit
serve: async func(input: stream<u8>) -> stream<u8>;
```

PHP passes a generator that reads from the socket, and writes every chunk of the returned stream back to it. The component parses RESP2, the protocol of Redis, runs the commands and writes the replies, so `redis-cli` works against it:

```sh
php examples/kv-server/server.php
redis-cli -p 6380 set greeting hello
redis-cli -p 6380 get greeting
```

The server listens on 127.0.0.1, port 6380 or the port given as the first argument. Commands also work as plain lines, so `nc` will do as a client:

```sh
printf 'SET visits 1\r\nINCR visits\r\nQUIT\r\n' | nc 127.0.0.1 6380
```

It knows PING, ECHO, SET with an optional EX in seconds, GET, DEL, EXISTS, INCR, KEYS with `*` and `?` patterns, DBSIZE and QUIT. COMMAND answers with an empty list, which is enough for `redis-cli` in interactive mode, as it asks for the command docs when it starts. Anything else gets an error reply. The data lives in the component instance, so it survives from one connection to the next while `server.php` runs, and is gone when it stops.

`src/lib.rs` is plain Rust without dependencies besides wit-bindgen. Each chunk PHP hands over goes into a buffer, the component runs every complete command in it and writes their replies in one go. A command split over several chunks waits in the buffer for the rest, and several commands in one chunk (pipelining) get their replies together.

## Requests and replies

The component asks for the next input chunk only when it has answered what it has. Reading the returned stream gives PHP the reply before the generator is asked for more input, so the generator's blocking `fread` never holds up a reply the client is waiting for. A request and its reply take turns this way without any extra code in PHP.

## One connection at a time

`server.php` serves one connection at a time, because PHP runs on one thread and `serve` returns only when the client sent QUIT or closed the connection. A second client can connect meanwhile, and waits in the listen queue until the first one is done. That suits a demo and a local tool, not many clients at once.

The component gets no listening socket of its own: the extension refuses listening in `wasi:sockets` (see `tcpHosts` in the [main README](../../README.md)). A component that listened itself would block the PHP thread while it waits for clients, and PHP-FPM workers would compete for the same port. With PHP holding the sockets, the component only sees bytes in and bytes out, and needs no network permissions at all.

## Running it

Install the extension as described in the [main README](../../README.md). The compiled `kv-server.wasm` is checked in, so running it needs no Rust. With the server running, `check.php` talks to it with a small RESP client and checks the replies, including a pipelined batch, a command split over two writes and a key that expires:

```sh
php examples/kv-server/server.php 6380 &
php examples/kv-server/check.php 6380
```

To rebuild the component from `src/lib.rs` and `wit/kv.wit`, run `./build.sh`, which needs Rust from [rustup.rs](https://rustup.rs). The parser has unit tests that run on the host with `cargo test`.
