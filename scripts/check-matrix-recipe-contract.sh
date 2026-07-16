#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
justfile="$repo_root/justfile"
cdp_runner="$repo_root/scripts/run-cdp-wasm-pack-test.sh"
focus_guard="$repo_root/scripts/run-with-focus-guard.sh"
safari_runner="$repo_root/scripts/run-safari-command.sh"
gitea_workflow="$repo_root/.gitea/workflows/smoke.yml"

require_recipe() {
    local recipe="$1"
    if ! grep -Eq "^${recipe}([[:space:]]|:)" "$justfile"; then
        printf 'Browser matrix is missing recipe %q.\n' "$recipe" >&2
        exit 1
    fi
}

for recipe in \
    test-edge \
    test-idb-edge \
    test-idb-safari \
    test-idb-iphone-safari \
    test-idb-android-chrome; do
    require_recipe "$recipe"
done

if [[ ! -x "$cdp_runner" ]]; then
    echo 'The configurable CDP wasm-pack runner is missing or not executable.' >&2
    exit 1
fi

if [[ ! -x "$focus_guard" ]]; then
    echo 'The Safari focus guard is missing or not executable.' >&2
    exit 1
fi

if [[ ! -x "$safari_runner" ]]; then
    echo 'The isolated Safari command runner is missing or not executable.' >&2
    exit 1
fi

for needle in \
    '/usr/bin/safaridriver' \
    'Safari .*--automation' \
    'run-with-focus-guard.sh'; do
    if ! grep -F -q -- "$needle" "$safari_runner"; then
        printf 'Safari command runner is missing %q.\n' "$needle" >&2
        exit 1
    fi
done

if ! grep -F -q -- 'SAFARI_FOCUS_GUARD' "$focus_guard"; then
    echo 'The Safari focus guard is missing its opt-out control.' >&2
    exit 1
fi

if ! grep -F -q -- 'scripts/run-with-focus-guard.sh' "$justfile"; then
    echo 'Safari recipes do not use the focus guard.' >&2
    exit 1
fi

for native_gate in \
    'cargo test -p pagedb-opfs-harness --lib' \
    'cargo test -p pagedb-opfs-harness --test readme_matrix'; do
    if ! grep -F -q -- "$native_gate" "$justfile"; then
        printf 'Native test gate is missing %q.\n' "$native_gate" >&2
        exit 1
    fi
done

if ! grep -F -q -- \
    'cargo run -p rust-browser-proofs --features runner -- -- cargo check' \
    "$gitea_workflow"; then
    echo 'Gitea smoke must enable the runner feature before invoking the command runner.' >&2
    exit 1
fi

for needle in \
    'NO_HEADLESS=1' \
    'WASM_BINDGEN_TEST_ADDRESS' \
    'scripts/cdp-browser-test.mjs' \
    '--user-data-dir='; do
    if ! grep -F -q -- "$needle" "$cdp_runner"; then
        printf 'CDP wasm-pack runner is missing %q.\n' "$needle" >&2
        exit 1
    fi
done

echo 'matrix_recipe_contract=ok'
