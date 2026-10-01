#!/usr/bin/env sh
# Rebuilds markdown.wasm, a WebAssembly component, from src/lib.rs and wit/. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/markdown.wasm markdown.wasm
echo "markdown.wasm rebuilt ($(wc -c < markdown.wasm) bytes)"
