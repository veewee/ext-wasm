#!/usr/bin/env sh
# Rebuilds component-http.wasm, a WASI preview2 command component, from
# src/main.rs. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/component-http.wasm component-http.wasm
echo "component-http.wasm rebuilt ($(wc -c < component-http.wasm) bytes)"
