#!/usr/bin/env sh
# Downloads the wasm build of oxipng (Apache-2.0) from the @jsquash/oxipng npm package.
set -eu

URL="https://registry.npmjs.org/@jsquash/oxipng/-/oxipng-2.3.0.tgz"
SHA256="e7d523bae6803574dd0e28296b7239f0d4689b0c195a9fbc4ff7657834c3196d"
DIST="$(cd "$(dirname "$0")" && pwd)/dist"

mkdir -p "$DIST"
curl -fsSL -o "$DIST/oxipng.tgz.part" "$URL"

if command -v sha256sum > /dev/null; then
  actual=$(sha256sum "$DIST/oxipng.tgz.part" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$DIST/oxipng.tgz.part" | cut -d' ' -f1)
fi
if [ "$actual" != "$SHA256" ]; then
  rm -f "$DIST/oxipng.tgz.part"
  echo "checksum mismatch: expected $SHA256, got $actual" >&2
  exit 1
fi

tar -xzf "$DIST/oxipng.tgz.part" -C "$DIST" package/codec/pkg/squoosh_oxipng_bg.wasm
mv "$DIST/package/codec/pkg/squoosh_oxipng_bg.wasm" "$DIST/oxipng.wasm"
rm -r "$DIST/package" "$DIST/oxipng.tgz.part"
echo "oxipng.wasm is in $DIST/oxipng.wasm"
