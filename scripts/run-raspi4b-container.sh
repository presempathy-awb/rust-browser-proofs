#!/usr/bin/env bash
set -euo pipefail

if (( $# < 1 )); then
    echo 'usage: run-raspi4b-container.sh <prepare|status|test> [report-path]' >&2
    exit 2
fi

mode="$1"
report_path="${2:-}"
image="${RUST_BROWSER_PROOFS_RASPI4_IMAGE:-rust-browser-proofs-raspi4b:local}"
volume_dir="${RUST_BROWSER_PROOFS_RASPI4_VOLUME_DIR:-$HOME/.volumes/rust-browser-proofs/raspi4b-model}"

case "$volume_dir" in
    /*) ;;
    *) echo 'RUST_BROWSER_PROOFS_RASPI4_VOLUME_DIR must be an absolute path.' >&2; exit 2 ;;
esac
mkdir -p "$volume_dir"

common_options=(
    run --rm
    --read-only
    --cap-drop ALL
    --security-opt no-new-privileges:true
    --pids-limit 256
    --memory 3g
    --cpus 4
    --tmpfs "/tmp:rw,noexec,nosuid,size=64m"
    --user "$(id -u):$(id -g)"
    --env HOME=/tmp
    --env RUST_BROWSER_PROOFS_RASPI4_VOLUME_DIR=/volume
    --env "RUST_BROWSER_PROOFS_RASPI4_HOST_VOLUME_DIR=$volume_dir"
    --env "RUST_BROWSER_PROOFS_RASPI4_BOOT_TIMEOUT_SECONDS=${RUST_BROWSER_PROOFS_RASPI4_BOOT_TIMEOUT_SECONDS:-90}"
    --env "RUST_BROWSER_PROOFS_RASPI4_NICE=${RUST_BROWSER_PROOFS_RASPI4_NICE:-15}"
    --mount "type=bind,source=$volume_dir,target=/volume"
)

case "$mode" in
    prepare)
        exec docker "${common_options[@]}" --network bridge "$image" prepare
        ;;
    status)
        exec docker "${common_options[@]}" --network none "$image" status
        ;;
    test)
        [[ -n "$report_path" ]] || { echo 'test mode requires an absolute report path.' >&2; exit 2; }
        case "$report_path" in
            /*) ;;
            *) echo 'test report path must be absolute.' >&2; exit 2 ;;
        esac
        report_dir="$(dirname "$report_path")"
        report_name="$(basename "$report_path")"
        mkdir -p "$report_dir"
        docker "${common_options[@]}" --network bridge "$image" prepare
        exec docker "${common_options[@]}" \
            --network none \
            --mount "type=bind,source=$report_dir,target=/reports" \
            "$image" test --report "/reports/$report_name"
        ;;
    *)
        printf 'Unknown Raspberry Pi 4 container mode: %s\n' "$mode" >&2
        exit 2
        ;;
esac
