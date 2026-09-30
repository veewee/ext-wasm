#!/usr/bin/env sh
# Rebuilds markdown.wasm from src/lib.rs. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/markdown.wasm markdown.wasm
echo "markdown.wasm rebuilt ($(wc -c < markdown.wasm) bytes)"
