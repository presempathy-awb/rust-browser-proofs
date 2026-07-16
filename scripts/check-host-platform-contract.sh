#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
justfile="$repo_root/justfile"
matrix="$repo_root/docs/host-platform-matrix.md"
container_runner="$repo_root/scripts/run-browser-container.sh"

if ! grep -Eq '^install-wasm32-unknown-unknown:' "$justfile"; then
    echo 'justfile is missing install-wasm32-unknown-unknown.' >&2
    exit 1
fi

if ! grep -F -q -- 'rustup target add wasm32-unknown-unknown' "$justfile"; then
    echo 'The Wasm target recipe does not install wasm32-unknown-unknown through Rustup.' >&2
    exit 1
fi

if [[ ! -f "$matrix" ]]; then
    echo 'docs/host-platform-matrix.md is missing.' >&2
    exit 1
fi

if [[ ! -x "$container_runner" ]]; then
    echo 'The host-backed browser container runner is missing or not executable.' >&2
    exit 1
fi

for needle in \
    'RUST_BROWSER_PROOFS_CONTAINER_CACHE_DIR' \
    'CARGO_TARGET_DIR' \
    '/home/browser/.cargo' \
    '/home/browser/.cargo-target'; do
    if ! grep -F -q -- "$needle" "$container_runner"; then
        printf 'Browser container runner is missing %q.\n' "$needle" >&2
        exit 1
    fi
done

if [[ "$(grep -F -c -- 'scripts/run-browser-container.sh' "$justfile")" -lt 3 ]]; then
    echo 'Container check, Chrome, and Firefox recipes must share the host-backed runner.' >&2
    exit 1
fi

for platform in \
    'macOS' \
    'Windows' \
    'Debian Linux' \
    'Ubuntu Linux' \
    'Manjaro Linux' \
    'Raspberry Pi OS'; do
    if ! grep -F -q -- "$platform" "$matrix"; then
        printf 'Host platform matrix is missing %q.\n' "$platform" >&2
        exit 1
    fi
done

echo 'host_platform_contract=ok'
