# Installs a released ext-wasm with PIE into a stock PHP image, the way a user
# would, and runs smoke.php against it. See run.sh.
ARG PHP_IMAGE=php:8.4-cli
FROM ${PHP_IMAGE}

# Only the Alpine image builds from source, which needs Rust and clang.
ARG SOURCE_BUILD=0
RUN if [ "$SOURCE_BUILD" = 1 ]; then \
        apk add --no-cache $PHPIZE_DEPS clang-dev rust cargo; \
    fi

COPY --from=ghcr.io/php/pie:bin /pie /usr/bin/pie

ARG VERSION
# Stock Debian PHP images lack unzip and libtool, which PIE asks for; this is
# what a Dockerfile installing ext-wasm needs too.
RUN pie install --auto-install-build-tools "veewee/ext-wasm:${VERSION}"

COPY smoke.php /smoke.php
CMD ["php", "/smoke.php"]
