#!/usr/bin/env sh
# Downloads jacobenget/doom.wasm (GPL-2.0), which embeds the shareware DOOM1.WAD.
set -eu

URL="https://github.com/jacobenget/doom.wasm/releases/download/v0.1.0/doom-v0.1.0.wasm"
SHA256="8edfe49a7583fd975199969302d8e9adcf8e714d0af72bf3e672f991fd810faa"
TARGET="$(cd "$(dirname "$0")" && pwd)/dist/doom.wasm"

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
echo "doom.wasm is in $TARGET"
