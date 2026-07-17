#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mise_config="$repo_root/.mise.toml"
workflow="$repo_root/.gitea/workflows/smoke.yml"
manifest="$repo_root/harness/Cargo.toml"
justfile="$repo_root/justfile"
recovery_doc="$repo_root/docs/setup-recovery.md"
lefthook_config="$repo_root/lefthook.yml"

if grep -F -q -- '"aqua:rustwasm/wasm-pack" = "latest"' "$mise_config" ||
    ! grep -F -q -- '"aqua:rustwasm/wasm-pack" = "0.15.0"' "$mise_config"; then
    echo 'Mise must pin wasm-pack 0.15.0 instead of following latest.' >&2
    exit 1
fi

for pin in \
    'rust = "1.95.0"' \
    'jj = "0.43.0"' \
    'just = "1.52.0"' \
    'lefthook = "2.1.9"'; do
    if ! grep -F -q -- "$pin" "$mise_config"; then
        printf 'Mise is missing exact tool pin %q.\n' "$pin" >&2
        exit 1
    fi
done

if grep -F -q -- 'branch = "main"' "$manifest" ||
    ! grep -F -q -- 'rev = "214113a7f489d9eb96a45b9eaa4a501ea95f18f5"' "$manifest"; then
    echo 'The hosted PageDB dependency must use the reviewed immutable revision.' >&2
    exit 1
fi

if grep -Eq 'actions/checkout@v[0-9]+' "$workflow" ||
    ! grep -F -q -- 'actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5' "$workflow"; then
    echo 'Gitea Actions checkout must use an immutable commit.' >&2
    exit 1
fi

if grep -F -q -- 'sh.rustup.rs' "$workflow"; then
    echo 'CI must not execute the mutable rustup network installer.' >&2
    exit 1
fi

for rustup_bootstrap in \
    'rustup/archive/1.29.0/x86_64-unknown-linux-gnu/rustup-init' \
    '4acc9acc76d5079515b46346a485974457b5a79893cfb01112423c89aeb5aa10' \
    'sha256sum --check'; do
    if ! grep -F -q -- "$rustup_bootstrap" "$workflow"; then
        printf 'Gitea Actions is missing verified Rustup bootstrap input %q.\n' "$rustup_bootstrap" >&2
        exit 1
    fi
done

if grep -F -q -- "\"\$rustup_init\" --yes" "$workflow" ||
    ! grep -F -q -- "\"\$rustup_init\" -y --profile minimal" "$workflow"; then
    echo 'Gitea Actions must use the Rustup 1.29.0 noninteractive -y flag.' >&2
    exit 1
fi

for route in \
    'ssh-keyscan -p 2222 gitea' \
    'ssh://git@gitea:2222/'; do
    if ! grep -F -q -- "$route" "$workflow"; then
        printf 'Gitea Actions is missing the current internal SSH route %q.\n' "$route" >&2
        exit 1
    fi
done

for needle in \
    'export RUSTUP_TOOLCHAIN := "1.95.0"' \
    'export PATH := rustup_bin + ":" + env_var("PATH")'; do
    if ! grep -F -q -- "$needle" "$justfile"; then
        printf 'Just recipes do not enforce the pinned Rustup toolchain: missing %q.\n' "$needle" >&2
        exit 1
    fi
done

for contract in \
    check-android-recipe-contract.sh \
    check-iphone-chrome-recipe-contract.sh \
    check-matrix-recipe-contract.sh \
    check-host-platform-contract.sh \
    check-setup-durability-contract.sh; do
    if ! grep -F -q -- "$contract" "$workflow"; then
        printf 'Gitea Actions does not enforce %s.\n' "$contract" >&2
        exit 1
    fi
done

for task in \
    setup-status \
    prepare-raspi4b-model \
    raspi4b-model-status \
    test-raspi4b-model \
    security-raspi4b-image; do
    if ! grep -F -q -- "[tasks.$task]" "$mise_config"; then
        printf 'Mise is missing the durable %s task.\n' "$task" >&2
        exit 1
    fi
done

if grep -Eq '^run = "just ' "$mise_config"; then
    echo 'Mise tasks must invoke Just through mise exec so host PATH cannot bypass the pin.' >&2
    exit 1
fi

if grep -Eq '^[[:space:]]+run: just ' "$lefthook_config"; then
    echo 'Lefthook commands must invoke Just through Mise so host PATH cannot bypass the pin.' >&2
    exit 1
fi

if [[ "$(grep -F -c -- '-$$-raspi4b-model' "$justfile")" -lt 2 ]]; then
    echo 'Raspberry Pi report names must include the process ID to avoid collisions.' >&2
    exit 1
fi

if [[ ! -f "$recovery_doc" ]]; then
    echo 'docs/setup-recovery.md is missing.' >&2
    exit 1
fi

for needle in \
    'Clean-machine bootstrap' \
    'just setup-status' \
    'RUST_BROWSER_PROOFS_RASPI4_VOLUME_DIR' \
    'report-cache.sqlite3' \
    'Home Assistant'; do
    if ! grep -F -q -- "$needle" "$recovery_doc"; then
        printf 'Setup recovery documentation is missing %q.\n' "$needle" >&2
        exit 1
    fi
done

echo 'setup_durability_contract=ok'
