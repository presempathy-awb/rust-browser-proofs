#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo 'usage: run-safari-command.sh bundle-id[,bundle-id...] -- command [args...]' >&2
    exit 2
}

[[ $# -ge 3 ]] || usage
target_bundles="$1"
shift
[[ "$1" == '--' ]] || usage
shift

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

stop_automation_processes() {
    local pid=''
    local -a pids=()
    while IFS= read -r pid; do
        [[ -n "$pid" ]] && pids+=("$pid")
    done < <(pgrep -f '^/usr/bin/safaridriver( |$)|/Safari .*--automation' || true)

    ((${#pids[@]} == 0)) && return 0
    kill "${pids[@]}" 2>/dev/null || true
    for _ in {1..20}; do
        local running=0
        for pid in "${pids[@]}"; do
            kill -0 "$pid" 2>/dev/null && running=1
        done
        ((running == 0)) && return 0
        sleep 0.1
    done
    kill -KILL "${pids[@]}" 2>/dev/null || true
}

stop_automation_processes
trap stop_automation_processes EXIT INT TERM
"$script_dir/run-with-focus-guard.sh" "$target_bundles" -- "$@"
