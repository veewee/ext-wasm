#!/usr/bin/env sh
# Rebuilds service-probe.wasm, a WebAssembly component, from src/lib.rs and wit/. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/service_probe.wasm service-probe.wasm
echo "service-probe.wasm rebuilt ($(wc -c < service-probe.wasm) bytes)"
