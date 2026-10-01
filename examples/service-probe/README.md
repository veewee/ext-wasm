# Service probe

This example is a Rust component that works out what listens on a TCP port, like a small `nmap -sV`. Given `host:port` targets, it connects to each and reports the service and the version or banner it gives away. PHP lets the component connect to exactly the targets it was given, through `tcpHosts`, so it cannot reach any other port or host, whatever the code inside does. A target given by name allows every address that name resolves to.

```sh
php examples/service-probe/probe.php localhost:8080 localhost:6379 localhost:5432 www.php.net:443
```

A run on a Mac with `php -S` on port 8080, a stand-in for Redis on 6379 and nothing on 5432:

```
localhost:8080               HTTP        HTTP/1.0 404 Not Found (127.0.0.1:8080, connected in 0 ms)
localhost:6379               Redis       answers PING (127.0.0.1:6379, connected in 0 ms)
localhost:5432               -           cannot connect, 127.0.0.1:5432: Connection refused (os error 14)
www.php.net:443              HTTP        HTTP/1.1 400 Bad Request, openresty (207.211.214.145:443, connected in 39 ms)
```

The last line is a web server that answers a plain HTTP request on its HTTPS port with an error. A server that only speaks TLS shows up as `TLS` when it answers with a TLS alert, and as `unknown` when it closes the connection without a reply, as `openssl s_server` does.

`src/lib.rs` is plain Rust with `std::net::TcpStream`, which on `wasm32-wasip2` goes through `wasi:sockets`. Services that speak first are read from their greeting: SSH, SMTP, FTP, POP3, IMAP, and MySQL or MariaDB with their version. For the others the component opens one connection per guess and sends a small request: `PING` for Redis, PostgreSQL's SSLRequest, and `HEAD /` for HTTP, whose status line and `Server` header it reports. A TLS alert in reply means the port expects a TLS handshake first. Every connect and read waits at most two seconds, set by `probe.php`, because the extension cannot interrupt a connect that hangs.

The target names are resolved by the component, to know where to connect, and again by the extension on every connect, to check the address against `tcpHosts`. That is also why a name in `tcpHosts` allows every address the name resolves to.

One detail of Rust on `wasm32-wasip2`: `TcpStream::connect_timeout` returns `Ok` for a refused connection, so `connect()` in `src/lib.rs` asks the stream for its error before using it.

## Running it

Install the extension as described in the [main README](../../README.md). The compiled `service-probe.wasm` is checked in, so running it needs no Rust. To rebuild it from `src/lib.rs` and `wit/probe.wit`, run `./build.sh`, which needs Rust from [rustup.rs](https://rustup.rs).
