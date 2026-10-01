# syntax=docker/dockerfile:1
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
# The official Debian images have neither the zip extension nor unzip, which
# PIE needs to unpack a prebuilt binary; a Dockerfile installing ext-wasm
# needs this too. Alpine ships unzip in busybox.
RUN if command -v apt-get > /dev/null; then \
        apt-get update && apt-get install -y --no-install-recommends unzip && rm -rf /var/lib/apt/lists/*; \
    fi
# PIE looks up the prebuilt binary through the GitHub API, which allows 60
# requests an hour without a token, and builds from source when the lookup
# fails. PIE reads the token from Composer's COMPOSER_AUTH, not GITHUB_TOKEN.
# It comes in as a secret, so it stays out of the image.
RUN --mount=type=secret,id=github_token,env=GITHUB_TOKEN \
    if [ -n "${GITHUB_TOKEN:-}" ]; then \
        export COMPOSER_AUTH="{\"github-oauth\":{\"github.com\":\"$GITHUB_TOKEN\"}}"; \
    fi; \
    pie install "veewee/ext-wasm:${VERSION}"

COPY smoke.php /smoke.php
CMD ["php", "/smoke.php"]
