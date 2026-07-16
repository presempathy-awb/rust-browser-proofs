#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
justfile="$repo_root/justfile"

assert_count() {
    local expected="$1"
    local needle="$2"
    local actual
    actual="$(grep -F -c -- "$needle" "$justfile" || true)"
    if [[ "$actual" != "$expected" ]]; then
        printf 'Expected %s Android recipes to contain %q, found %s.\n' \
            "$expected" "$needle" "$actual" >&2
        exit 1
    fi
}

assert_count 2 'am set-debug-app --persistent com.android.chrome'
assert_count 2 "settings get global debug_app | tr -d '\\r'"
assert_count 2 'shell am clear-debug-app'

echo 'android_recipe_contract=ok'
