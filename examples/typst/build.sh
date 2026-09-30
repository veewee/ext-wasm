#!/usr/bin/env sh
# Builds dist/typst.wasm from src/lib.rs. Needs Rust from rustup.rs, and takes
# a few minutes the first time.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
mkdir -p dist
cp target/wasm32-unknown-unknown/release/typst_example.wasm dist/typst.wasm
echo "dist/typst.wasm built ($(wc -c < dist/typst.wasm) bytes)"
