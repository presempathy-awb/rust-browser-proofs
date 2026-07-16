#!/usr/bin/env bash
set -euo pipefail

if (( $# < 3 )); then
    echo 'usage: run-browser-container.sh <image> [docker-options...] -- <command> [args...]' >&2
    exit 2
fi

image="$1"
shift
docker_options=()
while (( $# > 0 )) && [[ "$1" != '--' ]]; do
    docker_options+=("$1")
    shift
done

if (( $# == 0 )); then
    echo 'run-browser-container.sh requires -- before the container command.' >&2
    exit 2
fi
shift
if (( $# == 0 )); then
    echo 'run-browser-container.sh requires a container command.' >&2
    exit 2
fi

architecture="$(docker image inspect "$image" --format '{{.Architecture}}')"
cache_base="${RUST_BROWSER_PROOFS_CONTAINER_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/cache}/rust-browser-proofs/container-$architecture}"
if [[ "$cache_base" != /* ]]; then
    echo 'RUST_BROWSER_PROOFS_CONTAINER_CACHE_DIR must be an absolute path.' >&2
    exit 2
fi

cargo_cache="$cache_base/cargo"
target_cache="$cache_base/target"
mkdir -p "$cargo_cache" "$target_cache"

# The image runs as uid 10001. Prepare only these generated cache directories
# as root, then execute the proof as the image's unprivileged browser user.
docker run --rm --user root \
    --mount "type=bind,source=$cargo_cache,target=/home/browser/.cargo" \
    --mount "type=bind,source=$target_cache,target=/home/browser/.cargo-target" \
    "$image" \
    chown --recursive 10001:10001 \
        /home/browser/.cargo \
        /home/browser/.cargo-target

run_options=(run --rm)
if (( ${#docker_options[@]} > 0 )); then
    run_options+=("${docker_options[@]}")
fi

exec docker "${run_options[@]}" \
    --mount "type=bind,source=$cargo_cache,target=/home/browser/.cargo" \
    --mount "type=bind,source=$target_cache,target=/home/browser/.cargo-target" \
    --env CARGO_TARGET_DIR=/home/browser/.cargo-target \
    "$image" "$@"
