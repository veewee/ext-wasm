#!/usr/bin/env sh
# Rebuilds stream-gzip.wasm, an async WebAssembly component, from src/lib.rs and wit/. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/stream_gzip.wasm stream-gzip.wasm
echo "stream-gzip.wasm rebuilt ($(wc -c < stream-gzip.wasm) bytes)"
