#!/usr/bin/env sh
# Rebuilds html.wasm from src/lib.rs. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/html.wasm html.wasm
echo "html.wasm rebuilt ($(wc -c < html.wasm) bytes)"
