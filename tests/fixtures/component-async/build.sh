#!/usr/bin/env sh
# Rebuilds component-async.wasm, an async component, from
# src/lib.rs. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/component_async.wasm component-async.wasm
echo "component-async.wasm rebuilt ($(wc -c < component-async.wasm) bytes)"
