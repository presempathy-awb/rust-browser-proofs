#!/usr/bin/env bash
set -euo pipefail

if (( $# < 3 || $# > 4 )); then
    echo 'Usage: run-cdp-wasm-pack-test.sh <browser-binary> <project-dir> <test-suite> [features]' >&2
    exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
browser_binary="$1"
project_dir="$2"
test_suite="$3"
features="${4:-}"
wall_timeout="${CDP_WASM_WALL_TIMEOUT_SECONDS:-120}"
test_timeout="${CDP_WASM_TEST_TIMEOUT_SECONDS:-90}"
runner_pid=''
browser_pid=''
runner_log=''
browser_log=''
profile=''

free_port() {
    node -e 'const net=require("node:net");const server=net.createServer();server.listen(0,"127.0.0.1",()=>{console.log(server.address().port);server.close();});'
}

terminate_process_tree() {
    local parent="$1"
    local child=''
    for child in $(pgrep -P "$parent" 2>/dev/null || true); do
        terminate_process_tree "$child"
    done
    kill "$parent" 2>/dev/null || true
}

cleanup() {
    trap - EXIT INT TERM
    if [[ -n "$runner_pid" ]] && kill -0 "$runner_pid" 2>/dev/null; then
        terminate_process_tree "$runner_pid"
        wait "$runner_pid" 2>/dev/null || true
    fi
    if [[ -n "$browser_pid" ]] && kill -0 "$browser_pid" 2>/dev/null; then
        terminate_process_tree "$browser_pid"
        wait "$browser_pid" 2>/dev/null || true
    fi
    [[ -z "$runner_log" ]] || rm -f "$runner_log"
    [[ -z "$browser_log" ]] || rm -f "$browser_log"
    [[ -z "$profile" ]] || rm -rf "$profile"
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

if [[ ! -x "$browser_binary" ]]; then
    echo "CDP browser executable is missing: $browser_binary" >&2
    exit 1
fi
if [[ ! -f "$project_dir/Cargo.toml" ]]; then
    echo "Cargo project is missing: $project_dir" >&2
    exit 1
fi
for value in "$wall_timeout" "$test_timeout"; do
    if ! [[ "$value" =~ ^[0-9]+$ ]] || (( value < 1 )); then
        echo 'CDP timeouts must be positive integers.' >&2
        exit 1
    fi
done

test_port="${CDP_WASM_TEST_PORT:-$(free_port)}"
cdp_port="${CDP_BROWSER_PORT:-$(free_port)}"
if [[ "$test_port" == "$cdp_port" ]]; then
    echo 'CDP_WASM_TEST_PORT and CDP_BROWSER_PORT must differ.' >&2
    exit 1
fi
for port in "$test_port" "$cdp_port"; do
    if ! [[ "$port" =~ ^[0-9]+$ ]] || (( port < 1 || port > 65535 )); then
        echo 'CDP ports must be from 1 through 65535.' >&2
        exit 1
    fi
done

runner_log="$(mktemp "${TMPDIR:-/tmp}/rust-browser-proofs-cdp-runner.XXXXXX")"
browser_log="$(mktemp "${TMPDIR:-/tmp}/rust-browser-proofs-cdp-browser.XXXXXX")"
profile="$(mktemp -d "${TMPDIR:-/tmp}/rust-browser-proofs-cdp-profile.XXXXXX")"
toolchain="$(dirname "$(rustup which rustc)")"
wasm_pack_args=(test --headless --chrome --test "$test_suite")
if [[ -n "$features" ]]; then
    wasm_pack_args+=(--features "$features")
fi

(
    cd "$project_dir"
    PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" \
        NO_HEADLESS=1 WASM_BINDGEN_TEST_ADDRESS="127.0.0.1:$test_port" \
        WASM_BINDGEN_TEST_TIMEOUT="$test_timeout" \
        wasm-pack "${wasm_pack_args[@]}"
) >"$runner_log" 2>&1 &
runner_pid=$!

for _ in $(seq 1 "$wall_timeout"); do
    if curl --fail --silent --max-time 1 "http://127.0.0.1:$test_port/" >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$runner_pid" 2>/dev/null; then
        cat "$runner_log" >&2
        exit 1
    fi
    sleep 1
done
if ! curl --fail --silent --max-time 1 "http://127.0.0.1:$test_port/" >/dev/null 2>&1; then
    echo "The wasm-bindgen interactive server did not become ready after ${wall_timeout}s." >&2
    cat "$runner_log" >&2
    exit 1
fi

"$browser_binary" --headless=new --disable-gpu --disable-popup-blocking \
    --remote-debugging-port="$cdp_port" --user-data-dir="$profile" \
    --no-first-run --no-default-browser-check about:blank >"$browser_log" 2>&1 &
browser_pid=$!

for _ in $(seq 1 "$wall_timeout"); do
    if curl --fail --silent --max-time 1 "http://127.0.0.1:$cdp_port/json/version" >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$browser_pid" 2>/dev/null; then
        cat "$browser_log" >&2
        exit 1
    fi
    sleep 1
done
if ! curl --fail --silent --max-time 1 "http://127.0.0.1:$cdp_port/json/version" >/dev/null 2>&1; then
    echo "The CDP browser did not expose DevTools after ${wall_timeout}s." >&2
    cat "$browser_log" >&2
    exit 1
fi

if ! node "$repo_root/scripts/cdp-browser-test.mjs" \
    --cdp-url "http://127.0.0.1:$cdp_port" \
    --url "http://127.0.0.1:$test_port/" \
    --timeout-seconds "$test_timeout"; then
    cat "$runner_log" >&2
    exit 1
fi

echo "cdp_browser_test_passed=$test_suite"
