#!/usr/bin/env sh
# Downloads the official mago wasm build next to this script.
set -eu

VERSION="${1:-1.50.0}"
DIR="$(cd "$(dirname "$0")" && pwd)/dist"

mkdir -p "$DIR"
curl -fsSL "https://github.com/carthage-software/mago/releases/download/${VERSION}/mago-${VERSION}-wasm.tar.gz" \
  | tar -xz -C "$DIR"
echo "mago ${VERSION} wasm build is in ${DIR}"
