#!/usr/bin/env bash

set -euo pipefail

readonly test_port=8002
readonly timeout_seconds=90
readonly fixture_dir="${RUST_BROWSER_PROOFS_WORKSPACE:?}/fixtures/consumer-battery"
readonly runner_script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/run-opfs-battery.mjs"

runner_log="$(mktemp)"
runner_pid=""

cleanup() {
  if [[ -n "$runner_pid" ]] && kill -0 "$runner_pid" 2>/dev/null; then
    kill "$runner_pid" 2>/dev/null || true
    wait "$runner_pid" 2>/dev/null || true
  fi
  rm -f "$runner_log"
}

trap cleanup EXIT

(
  cd "$fixture_dir"
  NO_HEADLESS=1 WASM_BINDGEN_TEST_ADDRESS="127.0.0.1:$test_port" \
    rust-browser-proofs -- wasm-pack test --headless --chrome --test opfs_battery
) >"$runner_log" 2>&1 &
runner_pid=$!

set +e
node "$runner_script" \
  --url "http://127.0.0.1:$test_port/" \
  --timeout-seconds "$timeout_seconds"
result=$?
set -e

if (( result != 0 )); then
  cat "$runner_log" >&2
  exit "$result"
fi
