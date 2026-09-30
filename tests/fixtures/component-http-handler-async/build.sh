#!/usr/bin/env sh
# Rebuilds component-http-handler-async.wasm, a wasi:http proxy component, from
# src/lib.rs, with an import PHP can make suspending. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/component_http_handler_async.wasm component-http-handler-async.wasm
echo "component-http-handler-async.wasm rebuilt ($(wc -c < component-http-handler-async.wasm) bytes)"
