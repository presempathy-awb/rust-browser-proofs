#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
justfile="$repo_root/justfile"
matrix="$repo_root/docs/host-platform-matrix.md"
container_runner="$repo_root/scripts/run-browser-container.sh"
raspi4_runner="$repo_root/scripts/run-raspi4b-model-smoke.sh"
raspi4_container_runner="$repo_root/scripts/run-raspi4b-container.sh"
raspi4_dockerfile="$repo_root/Dockerfile.raspi4b"

if ! grep -Eq '^install-wasm32-unknown-unknown:' "$justfile"; then
    echo 'justfile is missing install-wasm32-unknown-unknown.' >&2
    exit 1
fi

if ! grep -F -q -- 'rustup target add wasm32-unknown-unknown' "$justfile"; then
    echo 'The Wasm target recipe does not install wasm32-unknown-unknown through Rustup.' >&2
    exit 1
fi

if ! grep -Eq '^check-windows-lock:' "$justfile"; then
    echo 'justfile is missing check-windows-lock.' >&2
    exit 1
fi

if ! grep -F -q -- 'x86_64-pc-windows-gnu' "$justfile"; then
    echo 'The Windows lock check does not compile the Windows target.' >&2
    exit 1
fi

if ! grep -F -q -- 'local Windows 11 ARM64 guest' "$justfile" || ! grep -F -q -- 'x86_64 GNU target' "$justfile"; then
    echo 'The Windows lock check does not document its architecture boundary.' >&2
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

if [[ ! -x "$raspi4_runner" ]]; then
    echo 'The Raspberry Pi 4 board-model smoke runner is missing or not executable.' >&2
    exit 1
fi

if [[ ! -x "$raspi4_container_runner" ]] || [[ ! -f "$raspi4_dockerfile" ]]; then
    echo 'The containerized Raspberry Pi 4 board-model lane is incomplete.' >&2
    exit 1
fi

if ! grep -Eq '^test-raspi4b-model([[:space:]][^:]*)?:' "$justfile" ||
    ! grep -Eq '^test-raspi4b-model-host' "$justfile"; then
    echo 'justfile is missing test-raspi4b-model.' >&2
    exit 1
fi

for needle in \
    '--read-only' \
    '--cap-drop' \
    '--security-opt' \
    '--network' \
    'RUST_BROWSER_PROOFS_RASPI4_VOLUME_DIR'; do
    if ! grep -F -q -- "$needle" "$raspi4_container_runner"; then
        printf 'Raspberry Pi 4 container runner is missing %q.\n' "$needle" >&2
        exit 1
    fi
done

for needle in \
    'RUST_BROWSER_PROOFS_RASPI4_VOLUME_DIR' \
    'raspi4b' \
    'ed05d403048f2956d9d3653acd996157363e94fe' \
    'a51c89fae919e457804fd8d83b3b53ff8ac1f5e8f84bf6850c130a2b434e8b95' \
    '75761b73c284e26623e4d1624bff13e67bce2ae620880efd81d6571a3739fcfb'; do
    if ! grep -F -q -- "$needle" "$raspi4_runner"; then
        printf 'Raspberry Pi 4 runner is missing %q.\n' "$needle" >&2
        exit 1
    fi
done

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

if [[ "$(grep -F -c -- 'scripts/run-browser-container.sh' "$justfile")" -lt 5 ]]; then
    echo 'All container build and browser recipes must share the host-backed runner.' >&2
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

if ! grep -F -q -- 'cargo test -p pagedb-opfs-harness --test windows_cross_process_lock' "$matrix"; then
    echo 'The host platform matrix is missing the Windows native lock command.' >&2
    exit 1
fi

if ! grep -F -q -- 'Windows 11 25H2 arm64 (local UTM guest)' "$matrix" ||
    ! grep -F -q -- 'Native lock verified 2026-07-17' "$matrix"; then
    echo 'The host platform matrix is missing the local Windows ARM64 evidence boundary.' >&2
    exit 1
fi

if ! grep -F -q -- 'QEMU raspi4b board model' "$matrix" ||
    ! grep -F -q -- 'Board-model boot evidence' "$matrix"; then
    echo 'The host platform matrix is missing the Raspberry Pi 4 simulation boundary.' >&2
    exit 1
fi

echo 'host_platform_contract=ok'
