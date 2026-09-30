#!/usr/bin/env sh
# Downloads CPython 3.12 built for WASI by VMware Labs (webassembly-language-runtimes, PSF license).
set -eu

URL="https://github.com/vmware-labs/webassembly-language-runtimes/releases/download/python/3.12.0%2B20231211-040d5a6/python-3.12.0.wasm"
SHA256="e5dc5a398b07b54ea8fdb503bf68fb583d533f10ec3f930963e02b9505f7a763"
TARGET="$(cd "$(dirname "$0")" && pwd)/dist/python.wasm"

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
echo "python.wasm is in $TARGET"
