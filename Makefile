help:                                                                           ## shows this help
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_\-\.]+:.*?## / {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

debug:																			## compiles a debug version of this extension
	cargo build;

compile:																		## compiles a release version of this extension
	cargo build -r;

stubs:																			## Generates stubs
	cargo php stubs;
	cat ext-wasm.stubs.php;

setup-ci:																		## Setup CI
	make compile

install:																		## Installs extension in current PHP
	cargo php install --release --yes;

reinstall:																		## Re-installs extension in current PHP
	make uninstall
	make install

uninstall:																		## Removes extension from current PHP
	cargo php remove --yes;

phpunit:																		## Run extension tests
	$(eval PHP_EXTENSION := $(shell make find-extension))
	php $(if $(PHP_EXTENSION),"-d extension=$(PHP_EXTENSION)",) ./tools/phpunit.phar;

find-extension:
	@find ./target/release ./target/debug -maxdepth 1 -type f \( -iname \*.so -o -iname \*.dll -o -iname \*.dylib \) 2>/dev/null | head -1;
