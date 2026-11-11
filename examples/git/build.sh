#!/usr/bin/env bash
# Rebuilds git.wasm, a WebAssembly component, from src/git.c, wit/ and libgit2.
# Needs curl, cmake and make. wasi-sdk, wit-bindgen, libgit2 and wasm-git's
# WASI patches for libgit2 are downloaded into build/ on first use.
set -euo pipefail

WASI_SDK_VERSION=34
WIT_BINDGEN_VERSION=0.62.0
LIBGIT2_VERSION=1.9.4
# wasm-git's WASI port of libgit2: stubs for users and processes, and a sysdir without HOME.
WASM_GIT_COMMIT=b629c73fb659780134a0ca12dbfbbfcac2bdb47f

cd "$(dirname "$0")"
HERE=$(pwd)
BUILD=$HERE/build

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)  SDK_PLATFORM=x86_64-linux; BINDGEN_PLATFORM=x86_64-linux ;;
    Linux-aarch64) SDK_PLATFORM=arm64-linux;  BINDGEN_PLATFORM=aarch64-linux ;;
    Darwin-arm64)  SDK_PLATFORM=arm64-macos;  BINDGEN_PLATFORM=aarch64-macos ;;
    Darwin-x86_64) SDK_PLATFORM=x86_64-macos; BINDGEN_PLATFORM=x86_64-macos ;;
    *) echo "Unsupported build platform: $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

WASI_SDK=$BUILD/wasi-sdk-$WASI_SDK_VERSION.0-$SDK_PLATFORM
WIT_BINDGEN=$BUILD/wit-bindgen-$WIT_BINDGEN_VERSION-$BINDGEN_PLATFORM/wit-bindgen
LIBGIT2=$BUILD/libgit2-$LIBGIT2_VERSION

mkdir -p "$BUILD"
if [ ! -d "$WASI_SDK" ]; then
    curl -sSfL "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-$WASI_SDK_VERSION/wasi-sdk-$WASI_SDK_VERSION.0-$SDK_PLATFORM.tar.gz" | tar -xz -C "$BUILD"
fi
if [ ! -x "$WIT_BINDGEN" ]; then
    curl -sSfL "https://github.com/bytecodealliance/wit-bindgen/releases/download/v$WIT_BINDGEN_VERSION/wit-bindgen-$WIT_BINDGEN_VERSION-$BINDGEN_PLATFORM.tar.gz" | tar -xz -C "$BUILD"
fi
if [ ! -d "$LIBGIT2" ]; then
    curl -sSfL "https://github.com/libgit2/libgit2/archive/refs/tags/v$LIBGIT2_VERSION.tar.gz" | tar -xz -C "$BUILD"
    curl -sSfL "https://github.com/petersalomonsen/wasm-git/archive/$WASM_GIT_COMMIT.tar.gz" | tar -xz -C "$BUILD"
    PATCHES=$BUILD/wasm-git-$WASM_GIT_COMMIT/libgit2patchedfiles/src
    rm "$LIBGIT2/src/libgit2/transports/http.c"
    cp -r "$PATCHES"/transports/* "$LIBGIT2/src/libgit2/transports/"
    cp -r "$PATCHES"/util/* "$LIBGIT2/src/util/"
    cp "$PATCHES/sysdir.c" "$LIBGIT2/src/libgit2/"
    for dir in src/libgit2 src/util; do
        echo 'set(CMAKE_C90_STANDARD_COMPILE_OPTION "-std=gnu90")' >> "$LIBGIT2/$dir/CMakeLists.txt"
    done
fi

mkdir -p "$BUILD/bindings" "$BUILD/libgit2"
"$WIT_BINDGEN" c wit --world git-reader --out-dir "$BUILD/bindings"

# NO_MMAP: WASI has no mmap, so libgit2 reads pack indexes into memory.
# GIT_WASI_NO_HTTP_CLIENT: read-only and local, so no wasi:http import.
cd "$BUILD/libgit2"
cmake "$LIBGIT2" \
    -DCMAKE_TOOLCHAIN_FILE="$WASI_SDK/share/cmake/wasi-sdk-p2.cmake" -DWASI_SDK_PREFIX="$WASI_SDK" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_FLAGS="-DNO_MMAP -DGIT_WASI_NO_HTTP_CLIENT" \
    -DREGEX_BACKEND=builtin -DUSE_HTTP_PARSER=builtin -DUSE_BUNDLED_ZLIB=ON \
    -DUSE_HTTPS=OFF -DUSE_SSH=OFF -DUSE_GSSAPI=OFF -DUSE_THREADS=OFF -DUSE_NSEC=OFF \
    -DBUILD_SHARED_LIBS=OFF -DBUILD_TESTS=OFF -DBUILD_CLI=OFF -DBUILD_EXAMPLES=OFF > cmake.log
make -j4 libgit2package > make.log

# The default 64 KiB stack is too small for libgit2.
cd "$HERE"
"$WASI_SDK/bin/clang" --target=wasm32-wasip2 -mexec-model=reactor -O2 -Wall \
    -I"$LIBGIT2/include" -I"$BUILD/bindings" \
    src/git.c "$BUILD/bindings/git_reader.c" "$BUILD/bindings/git_reader_component_type.o" \
    "$BUILD/libgit2/libgit2.a" -Wl,-z,stack-size=1048576 -o git.wasm
echo "git.wasm rebuilt ($(wc -c < git.wasm) bytes)"
