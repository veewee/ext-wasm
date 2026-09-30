dnl ext-wasm is a Rust extension built with ext-php-rs, so cargo does the
dnl compiling. This file connects the phpize, configure and make flow that PIE
dnl uses for source installs to a cargo build: `make` runs `cargo build --release`
dnl and copies the resulting library into modules/ as wasm.so.

PHP_ARG_ENABLE([wasm],
  [whether to enable WebAssembly support],
  [AS_HELP_STRING([--enable-wasm], [Enable WebAssembly support])],
  [yes])

if test "$PHP_WASM" != "no"; then
  AC_PATH_PROG(CARGO, cargo, no)
  if test "$CARGO" = "no"; then
    AC_MSG_ERROR([cargo is required to build ext-wasm from source, install Rust from https://rustup.rs])
  fi

  PHP_NEW_EXTENSION([wasm], [], [$ext_shared])

  CARGO_MANIFEST_DIR=$abs_srcdir
  PHP_SUBST([CARGO_MANIFEST_DIR])
  PHP_SUBST([CARGO])

  dnl cargo produces libwasm.dylib on macOS and libwasm.so elsewhere, while PHP
  dnl expects wasm.so on every Unix platform.
  cat >> Makefile.fragments <<'FRAGMENT'

cargo_build:
	cd $(CARGO_MANIFEST_DIR) && $(CARGO) build --release --locked
	test -d modules || mkdir modules
	cp $(CARGO_MANIFEST_DIR)/target/release/libwasm.dylib modules/wasm.so 2>/dev/null || \
		cp $(CARGO_MANIFEST_DIR)/target/release/libwasm.so modules/wasm.so

all: cargo_build
build-modules: cargo_build
FRAGMENT
fi
