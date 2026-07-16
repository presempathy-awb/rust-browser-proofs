#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo 'usage: run-with-focus-guard.sh bundle-id[,bundle-id...] -- command [args...]' >&2
    exit 2
}

[[ $# -ge 3 ]] || usage
target_bundles="$1"
shift
[[ "$1" == '--' ]] || usage
shift

if [[ "${SAFARI_FOCUS_GUARD:-1}" == '0' || "$(uname -s)" != 'Darwin' ]]; then
    exec "$@"
fi

frontmost_bundle() {
    osascript -e 'tell application "System Events" to get bundle identifier of first application process whose frontmost is true' 2>/dev/null
}

is_target_bundle() {
    local bundle="$1"
    local candidate=''
    local -a targets=()
    IFS=',' read -r -a targets <<<"$target_bundles"
    for candidate in "${targets[@]}"; do
        [[ "$bundle" == "$candidate" ]] && return 0
    done
    return 1
}

restore_bundle="$(frontmost_bundle || true)"
if [[ -z "$restore_bundle" ]]; then
    exec "$@"
fi
if is_target_bundle "$restore_bundle"; then
    exec "$@"
fi

guard_pid=''
cleanup() {
    trap - EXIT INT TERM
    if [[ -n "$guard_pid" ]]; then
        kill "$guard_pid" 2>/dev/null || true
        wait "$guard_pid" 2>/dev/null || true
    fi
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

(
    last_non_target="$restore_bundle"
    while true; do
        current="$(frontmost_bundle || true)"
        if [[ -n "$current" ]]; then
            if is_target_bundle "$current"; then
                /usr/bin/open -b "$last_non_target" >/dev/null 2>&1 || true
            else
                last_non_target="$current"
            fi
        fi
        sleep "${SAFARI_FOCUS_GUARD_INTERVAL_SECONDS:-0.35}"
    done
) &
guard_pid=$!

set +e
"$@"
status=$?
set -e
cleanup
exit "$status"
