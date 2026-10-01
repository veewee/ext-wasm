#!/usr/bin/env sh
# Rebuilds kv-server.wasm, an async WebAssembly component, from src/lib.rs and wit/. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/kv_server.wasm kv-server.wasm
echo "kv-server.wasm rebuilt ($(wc -c < kv-server.wasm) bytes)"
