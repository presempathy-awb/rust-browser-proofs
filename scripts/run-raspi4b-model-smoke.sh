#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
firmware_commit="ed05d403048f2956d9d3653acd996157363e94fe"
kernel_sha256="a51c89fae919e457804fd8d83b3b53ff8ac1f5e8f84bf6850c130a2b434e8b95"
dtb_sha256="75761b73c284e26623e4d1624bff13e67bce2ae620880efd81d6571a3739fcfb"
volume_dir="${RUST_BROWSER_PROOFS_RASPI4_VOLUME_DIR:-$HOME/.volumes/rust-browser-proofs/raspi4b-model}"
display_volume_dir="${RUST_BROWSER_PROOFS_RASPI4_HOST_VOLUME_DIR:-$volume_dir}"
firmware_dir="$volume_dir/firmware-$firmware_commit"
kernel="$firmware_dir/kernel8.img"
dtb="$firmware_dir/bcm2711-rpi-4-b.dtb"
mode="${1:-test}"
shift || true
report_path=""

while (( $# > 0 )); do
    case "$1" in
        --report)
            [[ $# -ge 2 ]] || { echo '--report requires a path.' >&2; exit 2; }
            report_path="$2"
            shift 2
            ;;
        *)
            printf 'Unknown argument: %s\n' "$1" >&2
            exit 2
            ;;
    esac
done

case "$volume_dir" in
    /*) ;;
    *) echo 'RUST_BROWSER_PROOFS_RASPI4_VOLUME_DIR must be an absolute path.' >&2; exit 2 ;;
esac
case "$volume_dir" in
    "$repo_root"|"$repo_root"/*)
        echo 'Raspberry Pi firmware and logs must live outside the repository.' >&2
        exit 2
        ;;
esac

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

verify_file() {
    local path="$1"
    local expected="$2"
    [[ -f "$path" ]] && [[ "$(sha256_file "$path")" == "$expected" ]]
}

download_verified() {
    local name="$1"
    local expected="$2"
    local path="$firmware_dir/$name"
    local url="https://raw.githubusercontent.com/raspberrypi/firmware/$firmware_commit/boot/$name"

    if verify_file "$path" "$expected"; then
        return
    fi

    mkdir -p "$firmware_dir"
    local partial="$path.partial.$$"
    if ! curl --fail --location --retry 3 --proto '=https' --tlsv1.2 "$url" --output "$partial"; then
        rm -f "$partial"
        exit 1
    fi
    if [[ "$(sha256_file "$partial")" != "$expected" ]]; then
        printf 'Checksum mismatch for %s from pinned firmware commit %s.\n' "$name" "$firmware_commit" >&2
        rm -f "$partial"
        exit 1
    fi
    mv "$partial" "$path"
}

prepare() {
    command -v curl >/dev/null 2>&1 || { echo 'curl is required.' >&2; exit 1; }
    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        echo 'sha256sum or shasum is required.' >&2
        exit 1
    fi
    download_verified kernel8.img "$kernel_sha256"
    download_verified bcm2711-rpi-4-b.dtb "$dtb_sha256"
    printf 'raspi4b_firmware_dir=%s\n' "$firmware_dir"
}

status() {
    printf 'raspi4b_volume_dir=%s\n' "$volume_dir"
    printf 'raspi4b_firmware_commit=%s\n' "$firmware_commit"
    if verify_file "$kernel" "$kernel_sha256"; then
        echo 'raspi4b_kernel=verified'
    else
        echo 'raspi4b_kernel=missing-or-invalid'
    fi
    if verify_file "$dtb" "$dtb_sha256"; then
        echo 'raspi4b_dtb=verified'
    else
        echo 'raspi4b_dtb=missing-or-invalid'
    fi
    if command -v qemu-system-aarch64 >/dev/null 2>&1; then
        qemu-system-aarch64 --version | head -1
        if qemu-system-aarch64 -machine help 2>/dev/null | grep -Eq '^raspi4b[[:space:]]'; then
            echo 'raspi4b_machine=available'
        else
            echo 'raspi4b_machine=unavailable'
        fi
    else
        echo 'qemu-system-aarch64=missing'
    fi
}

run_smoke() {
    prepare
    command -v qemu-system-aarch64 >/dev/null 2>&1 || {
        echo 'qemu-system-aarch64 is required; run this lane in the QEMU container or on a host with QEMU 11+.' >&2
        exit 1
    }
    if ! qemu-system-aarch64 -machine help 2>/dev/null | grep -Eq '^raspi4b[[:space:]]'; then
        echo 'The installed QEMU does not expose the raspi4b machine model.' >&2
        exit 1
    fi

    local timeout_seconds="${RUST_BROWSER_PROOFS_RASPI4_BOOT_TIMEOUT_SECONDS:-90}"
    local nice_level="${RUST_BROWSER_PROOFS_RASPI4_NICE:-15}"
    [[ "$timeout_seconds" =~ ^[1-9][0-9]*$ ]] || {
        echo 'RUST_BROWSER_PROOFS_RASPI4_BOOT_TIMEOUT_SECONDS must be a positive integer.' >&2
        exit 2
    }
    [[ "$nice_level" =~ ^([0-9]|1[0-9]|20)$ ]] || {
        echo 'RUST_BROWSER_PROOFS_RASPI4_NICE must be an integer from 0 through 20.' >&2
        exit 2
    }

    local run_id
    run_id="$(date -u +%s)-$$"
    local log_dir="$volume_dir/runs"
    local log_path="$log_dir/$run_id-qemu.log"
    mkdir -p "$log_dir"

    nice -n "$nice_level" qemu-system-aarch64 \
        -machine raspi4b \
        -accel tcg,thread=multi \
        -kernel "$kernel" \
        -dtb "$dtb" \
        -append 'earlycon=pl011,mmio32,0xfe201000 console=ttyAMA0,115200 console=tty1 loglevel=8' \
        -display none \
        -serial stdio \
        -monitor none \
        -no-reboot >"$log_path" 2>&1 &
    local qemu_pid=$!
    cleanup_qemu() {
        if kill -0 "$qemu_pid" 2>/dev/null; then
            kill -TERM "$qemu_pid" 2>/dev/null || true
        fi
        wait "$qemu_pid" 2>/dev/null || true
    }
    trap cleanup_qemu EXIT INT TERM

    local passed=0
    local _attempt
    for _attempt in $(seq 1 "$timeout_seconds"); do
        if grep -F -q -- 'Booting Linux on physical CPU' "$log_path" 2>/dev/null &&
            grep -F -q -- 'Linux version' "$log_path" 2>/dev/null &&
            grep -F -q -- 'Machine model: Raspberry Pi 4 Model B' "$log_path" 2>/dev/null; then
            passed=1
            break
        fi
        if ! kill -0 "$qemu_pid" 2>/dev/null; then
            break
        fi
        sleep 1
    done

    cleanup_qemu
    trap - EXIT INT TERM
    cat "$log_path"

    if [[ "$passed" != 1 ]]; then
        printf 'QEMU raspi4b did not emit all required boot markers within %s seconds. Log: %s\n' \
            "$timeout_seconds" "$log_path" >&2
        exit 1
    fi

    if [[ -z "$report_path" ]]; then
        local report_dir="${RUST_BROWSER_PROOFS_REPORT_DIR:-${XDG_CACHE_HOME:-$HOME/cache}/rust-browser-proofs/browser-tests}"
        report_path="$report_dir/$run_id-raspi4b-model-status.md"
    fi
    case "$report_path" in
        /*) ;;
        *) echo '--report must be an absolute path.' >&2; exit 2 ;;
    esac
    mkdir -p "$(dirname "$report_path")"

    local qemu_version kernel_version display_log_path
    qemu_version="$(qemu-system-aarch64 --version | head -1)"
    kernel_version="$(grep -F -- 'Linux version' "$log_path" | head -1 | tr -d '\r' | sed 's/^[^]]*] //')"
    display_log_path="$display_volume_dir${log_path#"$volume_dir"}"
    {
        echo '# Raspberry Pi 4 Board-Model Smoke Report'
        echo
        echo "- Status: **PASS**"
        echo "- Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo "- QEMU: \`$qemu_version\`"
        printf '%s\n' "- Machine: \`raspi4b\` (Raspberry Pi 4B revision 1.5 model)"
        echo '- Emulated CPU: Cortex-A72, four cores'
        echo "- Firmware commit: \`$firmware_commit\`"
        echo "- Kernel SHA-256: \`$kernel_sha256\`"
        echo "- DTB SHA-256: \`$dtb_sha256\`"
        echo "- Guest marker: \`$kernel_version\`"
        echo "- QEMU log: \`$display_log_path\`"
        echo
        echo '## Proof Boundary'
        echo
        echo "This proves that the pinned Raspberry Pi kernel reaches Linux under QEMU's \`raspi4b\` board model and identifies the guest as Raspberry Pi 4 Model B. It does not prove Raspberry Pi OS userland, browser behavior, OPFS, physical Raspberry Pi hardware, VideoCore/GPU behavior, SD-card durability, thermal behavior, or network behavior."
        echo
        echo 'QEMU does not currently implement the Raspberry Pi 4 PCIe root port or GENET Ethernet controller. Those gaps prevent promotion of this smoke result into a browser or network proof.'
    } >"$report_path"

    printf 'raspi4b_model_smoke=pass\nreport_path=%s\nqemu_log=%s\n' "$report_path" "$display_log_path"
}

case "$mode" in
    prepare) prepare ;;
    status) status ;;
    test) run_smoke ;;
    *) printf 'usage: %s [prepare|status|test] [--report /absolute/path]\n' "$0" >&2; exit 2 ;;
esac
