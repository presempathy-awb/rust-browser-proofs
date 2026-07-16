#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
justfile="$repo_root/justfile"
source_builder="$repo_root/scripts/build-iphone-chromium-source.sh"

require_text() {
    local needle="$1"
    if ! grep -F -q -- "$needle" "$justfile"; then
        printf 'iPhone Chrome recipe is missing %q.\n' "$needle" >&2
        exit 1
    fi
}

require_text 'com.google.chrome.ios'
require_text 'org.chromium.ost.chrome.ios.dev'
require_text 'IPHONE_CHROME_BUNDLE_ID'
require_text 'plutil -extract CFBundleIdentifier raw'
require_text 'IPHONE_CHROME_URL_SCHEME'
require_text 'build-iphone-chromium-source'
require_text 'run-iphone-chromium-source'
require_text 'FirstRunForceDisabled'
require_text '--disable-features=BuildExternalPrivacyContext'
# This contract asserts the literal recipe expression.
# shellcheck disable=SC2016
require_text 'xcrun simctl terminate booted "$bundle_id"'
require_text 'IPHONE_CHROME_STABILITY_SECONDS'
require_text 'iphone_chrome_survived_seconds'

require_source_text() {
    local needle="$1"
    if [[ ! -f "$source_builder" ]] || ! grep -F -q -- "$needle" "$source_builder"; then
        printf 'iPhone Chromium source builder is missing %q.\n' "$needle" >&2
        exit 1
    fi
}

# This contract asserts the literal default expression.
# shellcheck disable=SC2016
require_source_text '${CHROMIUM_IOS_ROOT:-$HOME/.volumes/chromium}'
require_source_text 'GIT_CACHE_PATH'
require_source_text 'ensure_bootstrap'
require_source_text 'chromium/ci/ios-simulator'
require_source_text 'source-revision.txt'
require_source_text 'CHROMIUM_REFRESH_REVISION'
require_source_text 'chromium_revision_source=persisted'
require_source_text 'app-files.sha256'
require_source_text 'CHROMIUM_HEARTBEAT_SECONDS'
require_source_text 'run_logged'
require_source_text 'printf '\''iphone Chromium source build: %s\n'\'' "$*" >&4'
require_source_text 'sandbox-exec'
require_source_text 'deny file-read*'
# shellcheck disable=SC2016
require_source_text '$HOME/node_modules'

if grep -F -q -- 'exec > >(tee' "$source_builder"; then
    echo 'iPhone Chromium source builder must not stream the full build log to the terminal.' >&2
    exit 1
fi

if grep -F -q -- 'get_app_container booted com.google.chrome >/dev/null' "$justfile"; then
    echo 'iPhone Chrome check must resolve the installed bundle instead of hard-coding Google Chrome.' >&2
    exit 1
fi

echo 'iphone_chrome_recipe_contract=ok'
