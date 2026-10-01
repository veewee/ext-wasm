#!/usr/bin/env sh
# Rebuilds component-wasi.wasm, a WASI preview2 command component, from
# src/main.rs. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/component-wasi.wasm component-wasi.wasm
echo "component-wasi.wasm rebuilt ($(wc -c < component-wasi.wasm) bytes)"
