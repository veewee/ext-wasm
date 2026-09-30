#!/usr/bin/env sh
# Rebuilds component-http-handler.wasm, a WASI preview2 command component, from
# src/lib.rs. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/component_http_handler.wasm component-http-handler.wasm
echo "component-http-handler.wasm rebuilt ($(wc -c < component-http-handler.wasm) bytes)"
