#!/usr/bin/env sh
# Checks that `pie install veewee/ext-wasm:<version>` works on stock PHP
# images, once the release is published (PIE does not see drafts).
#
#   tools/pie-check/run.sh 0.1.0             prebuilt binaries, Linux arm64 and x86_64
#   tools/pie-check/run.sh 0.1.0 --source    also an Alpine image, which builds from source
#
# x86_64 runs under emulation on an arm64 host, which is slow but works.
set -u

cd "$(dirname "$0")"
version=${1:?usage: run.sh <version> [--source]}
case $version in -*) echo "usage: run.sh <version> [--source]" >&2; exit 2 ;; esac
# A token keeps PIE within GitHub's API limit; without one, a few runs in an
# hour make PIE fall back to building from source, which fails here.
GITHUB_TOKEN=${GITHUB_TOKEN:-$(gh auth token 2> /dev/null)}
export GITHUB_TOKEN
failed=0
log=$(mktemp)
trap 'rm -f "$log"' EXIT

check() {
    image=$1 platform=$2 source=${3:-0}
    tag="ext-wasm-pie-check:$(echo "$image-$platform" | tr ':/' '--')"
    # No cache, so a replaced or removed release asset is downloaded again.
    if docker build --pull --no-cache --progress=plain --platform "$platform" --build-arg PHP_IMAGE="$image" \
            --build-arg VERSION="$version" --build-arg SOURCE_BUILD="$source" --secret id=github_token,env=GITHUB_TOKEN \
            -t "$tag" . > "$log" 2>&1 \
        && result=$(docker run --rm --platform "$platform" -e VERSION="$version" "$tag" 2>&1); then
        echo "ok    $image $platform: $result"
    else
        echo "FAIL  $image $platform"
        tail -20 "$log" | sed 's/^/      /'
        [ -n "${result:-}" ] && echo "$result" | sed 's/^/      /'
        failed=1
    fi
    result=
}

for platform in linux/arm64 linux/amd64; do
    for image in php:8.2-cli php:8.3-cli php:8.4-cli php:8.5-cli php:8.4-zts; do
        check "$image" "$platform"
    done
done
if [ "${2:-}" = --source ]; then
    check php:8.4-cli-alpine linux/arm64 1
fi
exit $failed
