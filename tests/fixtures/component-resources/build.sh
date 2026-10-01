#!/usr/bin/env sh
# Rebuilds counters.wasm, logger.wasm and composer.wasm, components with
# resources in both directions, from counters/, logger/, composer/ and wit/. Needs Rust from rustup.rs.
set -eu

cd "$(dirname "$0")"
rustup target add wasm32-wasip2
for crate in counters logger composer; do
    (cd "$crate" && cargo build --release --target wasm32-wasip2)
    cp "$crate/target/wasm32-wasip2/release/$crate.wasm" "$crate.wasm"
    echo "$crate.wasm rebuilt ($(wc -c < "$crate.wasm") bytes)"
done
