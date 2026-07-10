# pagedb-opfs - operational recipes
# Run `just` (no args) to list recipes.

set dotenv-load := true
set shell := ["bash", "-uc"]

default:
    @just --list

# One-time setup: tools, wasm target, git hooks
setup:
    mise install
    rustup target add wasm32-unknown-unknown
    lefthook install

# Browser suites (dedicated-worker OPFS tests). Browsers required locally.
# .tools/chromedriver (gitignored) must match the installed Chrome major
# version; wasm-pack's auto-fetched driver can drift ahead of the browser.
# Generous per-test timeout: suites are fast in isolation but share the
# machine with builds/review jobs; timing out under load is pure flake.
# The crash oracle embeds a second, self-contained wasm bundle (the
# sacrificial worker's driver) built from the harness lib itself.
build-driver:
    cd harness && wasm-pack build --dev --target no-modules --no-typescript --out-dir pkg-driver

check-chrome-driver:
    #!/usr/bin/env bash
    set -euo pipefail
    driver="{{justfile_directory()}}/.tools/chromedriver"
    if [[ ! -x "$driver" ]]; then
        echo "ChromeDriver is missing or not executable: $driver" >&2
        exit 1
    fi
    port=$((20000 + RANDOM % 20000))
    log="$(mktemp "${TMPDIR:-/tmp}/pagedb-opfs-chromedriver.XXXXXX")"
    pid=''
    cleanup() {
        if [[ -n "$pid" ]]; then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
        rm -f "$log"
    }
    trap cleanup EXIT
    "$driver" --port="$port" --verbose >"$log" 2>&1 &
    pid=$!
    for _ in {1..20}; do
        if curl --fail --silent --max-time 1 "http://127.0.0.1:$port/status" >/dev/null 2>&1; then
            exit 0
        fi
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
    done
    echo "ChromeDriver did not open its WebDriver port within two seconds: $driver" >&2
    if xattr -p com.apple.quarantine "$driver" >/dev/null 2>&1; then
        echo "The driver carries macOS quarantine metadata; this is diagnostic only." >&2
    fi
    echo "Inspect host execution policy and resource pressure; do not change trust settings automatically." >&2
    if [[ -s "$log" ]]; then
        cat "$log" >&2
    else
        echo "ChromeDriver produced no startup output." >&2
    fi
    exit 1

test-chrome: check-chrome-driver build-driver
    cd harness && WASM_BINDGEN_TEST_TIMEOUT=120 wasm-pack test --headless --chrome --chromedriver "{{justfile_directory()}}/.tools/chromedriver"

test-firefox: build-driver
    cd harness && WASM_BINDGEN_TEST_TIMEOUT=120 wasm-pack test --headless --firefox

# Local-only PageDB IDB fallback proof. Requires the gitignored Cargo patch
# to the `codex/idb-vfs-fallback` vendor branch; it is intentionally not CI.
test-idb-firefox:
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --firefox --test idb_spike && wasm-pack test --headless --firefox --test idb_store --features idb-vendor-spike && wasm-pack test --headless --firefox --test idb_vfs --features idb-vendor-spike && wasm-pack test --headless --firefox --test idb_receipt --features idb-vendor-spike && wasm-pack test --headless --firefox --test idb_cross_worker --features idb-vendor-spike'

test-browsers: test-chrome test-firefox

# Native-side tests (manifest codec, receipt reference, etc.)
test-native:
    cargo test --workspace

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Compile-check the harness for wasm32 without running browsers
wasm-check:
    cargo check -p pagedb-opfs-harness --target wasm32-unknown-unknown
