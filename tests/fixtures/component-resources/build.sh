#!/usr/bin/env sh
# Rebuilds counters.wasm and logger.wasm, components with resources in both
# directions, from counters/, logger/ and wit/. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
for crate in counters logger; do
    (cd "$crate" && cargo build --release --target wasm32-wasip2)
    cp "$crate/target/wasm32-wasip2/release/$crate.wasm" "$crate.wasm"
    echo "$crate.wasm rebuilt ($(wc -c < "$crate.wasm") bytes)"
done
