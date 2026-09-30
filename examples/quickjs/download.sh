#!/usr/bin/env sh
# Downloads the WASI build of QuickJS-ng (MIT), a small JavaScript engine.
set -eu

URL="https://github.com/quickjs-ng/quickjs/releases/download/v0.17.0/qjs-wasi.wasm"
SHA256="42a732a676ec2d93488c19411e0fad283bf72658fdad746f089914b523c783b1"
TARGET="$(cd "$(dirname "$0")" && pwd)/dist/qjs.wasm"

mkdir -p "$(dirname "$TARGET")"
curl -fsSL -o "$TARGET.part" "$URL"

if command -v sha256sum > /dev/null; then
  actual=$(sha256sum "$TARGET.part" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$TARGET.part" | cut -d' ' -f1)
fi
if [ "$actual" != "$SHA256" ]; then
  rm -f "$TARGET.part"
  echo "checksum mismatch: expected $SHA256, got $actual" >&2
  exit 1
fi

mv "$TARGET.part" "$TARGET"
echo "qjs.wasm is in $TARGET"
