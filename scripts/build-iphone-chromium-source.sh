#!/usr/bin/env bash
set -euo pipefail
exec 3>&1 4>&2

root="${CHROMIUM_IOS_ROOT:-$HOME/.volumes/chromium}"
depot_tools="$root/depot_tools"
checkout="$root/checkout"
source_dir="$checkout/src"
git_cache="$root/git-cache"
logs="$root/logs"
provenance="$root/provenance"
artifacts="$root/artifacts"
out_dir="$source_dir/out/Debug-iphonesimulator"

fail() {
    printf 'iphone Chromium source build: %s\n' "$*" >&4
    exit 1
}

case "$root" in
    "$HOME/code" | "$HOME/code/"*)
        fail "CHROMIUM_IOS_ROOT must not be under $HOME/code"
        ;;
esac
[[ "$root" != *' '* ]] || fail 'CHROMIUM_IOS_ROOT must not contain spaces'

for command in curl ditto git jq lipo plutil python3 sandbox-exec shasum xcodebuild xcrun; do
    command -v "$command" >/dev/null 2>&1 || fail "required command is missing: $command"
done

xcode_version="$(xcodebuild -version | awk 'NR == 1 { print $2 }')"
xcode_major="${xcode_version%%.*}"
[[ "$xcode_major" =~ ^[0-9]+$ ]] || fail "could not parse Xcode version: $xcode_version"
(( xcode_major >= 26 )) || fail "Xcode 26.0 or newer is required; found $xcode_version"
xcrun simctl list runtimes | grep -q 'iOS .* - com.apple.CoreSimulator.SimRuntime.iOS-' ||
    fail 'an iOS Simulator runtime is required'

mkdir -p "$root" "$logs" "$provenance" "$artifacts"
run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$"
log_path="$logs/source-build-$run_id.log"
denied_host_node_modules="$HOME/node_modules"
build_sandbox_profile="(version 1) (allow default) (deny file-read* (subpath \"$denied_host_node_modules\"))"
heartbeat_seconds="${CHROMIUM_HEARTBEAT_SECONDS:-30}"
[[ "$heartbeat_seconds" =~ ^[1-9][0-9]*$ ]] ||
    fail 'CHROMIUM_HEARTBEAT_SECONDS must be a positive integer'

exec >>"$log_path" 2>&1

report() {
    printf '%s\n' "$*"
    printf '%s\n' "$*" >&3
}

report_failure_tail() {
    printf 'chromium_failure_log_tail_begin\n' >&4
    tail -c 65536 "$log_path" | tr '\r' '\n' | tail -n 80 >&4
    printf 'chromium_failure_log_tail_end\n' >&4
}

run_logged() {
    local phase="$1"
    shift
    local started_at="$SECONDS"
    local next_heartbeat=$((SECONDS + heartbeat_seconds))
    local pid
    local rc=0

    report "chromium_phase=$phase status=running log_path=$log_path"
    "$@" &
    pid=$!
    while kill -0 "$pid" >/dev/null 2>&1; do
        sleep 1
        if ((SECONDS >= next_heartbeat)); then
            report "chromium_phase=$phase status=running elapsed_seconds=$((SECONDS - started_at))"
            next_heartbeat=$((SECONDS + heartbeat_seconds))
        fi
    done
    wait "$pid" || rc=$?
    if ((rc != 0)); then
        report "chromium_phase=$phase status=failed exit_code=$rc log_path=$log_path"
        report_failure_tail
        return "$rc"
    fi
    report "chromium_phase=$phase status=complete elapsed_seconds=$((SECONDS - started_at))"
}

report "chromium_ios_root=$root"
report "xcode_version=$xcode_version"
report "log_path=$log_path"
report "chromium_build_denied_path=$denied_host_node_modules"

if [[ ! -f "$checkout/.gclient" ]]; then
    minimum_free_gib="${CHROMIUM_MIN_FREE_GIB:-150}"
    [[ "$minimum_free_gib" =~ ^[0-9]+$ ]] || fail 'CHROMIUM_MIN_FREE_GIB must be an integer'
    available_kib="$(df -Pk "$root" | awk 'NR == 2 { print $4 }')"
    required_kib=$((minimum_free_gib * 1024 * 1024))
    report "available_before_fetch_gib=$((available_kib / 1024 / 1024))"
    (( available_kib >= required_kib )) ||
        fail "initial checkout requires the configured $minimum_free_gib GiB free-space guard"
fi

if [[ ! -d "$depot_tools/.git" ]]; then
    [[ ! -e "$depot_tools" ]] || fail "$depot_tools exists but is not a depot_tools Git checkout"
    git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git "$depot_tools"
fi
export PATH="$depot_tools:$PATH"
"$depot_tools/ensure_bootstrap"
export PATH="$depot_tools:$depot_tools/python-bin:$PATH"
export GIT_CACHE_PATH="$git_cache"

for command in autoninja fetch gclient; do
    command -v "$command" >/dev/null 2>&1 || fail "depot_tools command is missing: $command"
done

pin_path="$provenance/source-revision.txt"
refresh_revision="${CHROMIUM_REFRESH_REVISION:-0}"
[[ "$refresh_revision" == 0 || "$refresh_revision" == 1 ]] ||
    fail 'CHROMIUM_REFRESH_REVISION must be 0 or 1'

revision="${CHROMIUM_REVISION:-}"
revision_source=explicit
if [[ -z "$revision" && "$refresh_revision" == 0 && -s "$pin_path" ]]; then
    revision="$(tr -d '[:space:]' <"$pin_path")"
    revision_source=persisted
    report 'chromium_revision_source=persisted'
fi

if [[ -z "$revision" ]]; then
    revision_source=luci
    builder_request='{"predicate":{"builder":{"project":"chromium","bucket":"ci","builder":"ios-simulator"}},"pageSize":20}'
    builder_raw="$provenance/ios-simulator-builds-$run_id.raw.json"
    builder_json="$provenance/ios-simulator-builds-$run_id.json"
    curl -fsS \
        -H 'Content-Type: application/json' \
        -H 'Accept: application/json' \
        -X POST \
        --data "$builder_request" \
        https://cr-buildbucket.appspot.com/prpc/buildbucket.v2.Builds/SearchBuilds \
        >"$builder_raw"
    python3 - "$builder_raw" "$builder_json" <<'PY'
import json
import pathlib
import sys

raw = pathlib.Path(sys.argv[1]).read_text()
if raw.startswith(")]}'"):
    raw = raw[4:]
data = json.loads(raw)
pathlib.Path(sys.argv[2]).write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
PY
    revision="$(jq -r '.builds[] | select(.status == "SUCCESS") | .input.gitilesCommit.id' "$builder_json" | head -1)"
fi
[[ "$revision" =~ ^[0-9a-f]{40}$ ]] || fail "invalid Chromium revision: $revision"
printf '%s\n' "$revision" >"$pin_path"
printf 'chromium_revision=%s\n' "$revision"
printf 'chromium_revision_source=%s\n' "$revision_source"

if [[ ! -f "$checkout/.gclient" ]]; then
    if [[ -d "$checkout" ]] && find "$checkout" -mindepth 1 -maxdepth 1 | grep -q .; then
        fail "$checkout is non-empty but is not a Chromium gclient checkout"
    fi
    mkdir -p "$checkout" "$git_cache"
    fetch_checkout() {
        cd "$checkout"
        fetch --git-cache ios
    }
    run_logged fetch-ios-source fetch_checkout
fi

[[ -d "$source_dir/.git" ]] || fail "Chromium source checkout is missing: $source_dir"
run_logged fetch-pinned-revision git -C "$source_dir" fetch origin "$revision"
git -C "$source_dir" checkout --detach "$revision"
sync_checkout() {
    cd "$checkout"
    gclient sync --delete_unversioned_trees --force --revision "src@$revision"
}
run_logged sync-pinned-dependencies sync_checkout
resolved_revision="$(git -C "$source_dir" rev-parse HEAD)"
[[ "$resolved_revision" == "$revision" ]] ||
    fail "resolved source revision $resolved_revision does not match $revision"

depot_tools_revision="$(git -C "$depot_tools" rev-parse HEAD)"
printf '%s\n' "$depot_tools_revision" >"$provenance/depot-tools-revision.txt"

setup_gn() {
    cd "$source_dir"
    python3 ios/build/tools/setup-gn.py
}
run_logged setup-gn setup_gn

build_chromium() {
    local -a command=(autoninja -C "$out_dir")
    cd "$source_dir"
    if [[ -n "${CHROMIUM_BUILD_JOBS:-}" ]]; then
        command+=(-j "$CHROMIUM_BUILD_JOBS")
    fi
    command+=(chrome)
    sandbox-exec -p "$build_sandbox_profile" "${command[@]}"
}
run_logged build-chromium-app build_chromium

built_app="$out_dir/Chromium.app"
[[ -f "$built_app/Info.plist" ]] || fail "build did not produce $built_app"
bundle_id="$(plutil -extract CFBundleIdentifier raw -o - "$built_app/Info.plist")"
executable_name="$(plutil -extract CFBundleExecutable raw -o - "$built_app/Info.plist")"
[[ -f "$built_app/$executable_name" ]] || fail "bundle executable is missing: $executable_name"

artifact_dir="$artifacts/$revision"
artifact_app="$artifact_dir/Chromium.app"
if [[ ! -d "$artifact_app" ]]; then
    staging="$artifacts/.staging-$revision-$$"
    trap 'rm -rf "$staging"' EXIT
    mkdir -p "$staging"
    ditto "$built_app" "$staging/Chromium.app"
    mkdir -p "$artifact_dir"
    mv "$staging/Chromium.app" "$artifact_app"
    rmdir "$staging"
    trap - EXIT
fi

(
    cd "$artifact_dir"
    find Chromium.app -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 \
        >app-files.sha256
)

architectures="$(lipo -archs "$artifact_app/$executable_name")"
cat >"$artifact_dir/build-provenance.txt" <<EOF
source_repository=https://chromium.googlesource.com/chromium/src.git
source_revision=$revision
source_revision_source=$revision_source
public_builder=chromium/ci/ios-simulator
depot_tools_repository=https://chromium.googlesource.com/chromium/tools/depot_tools.git
depot_tools_revision=$depot_tools_revision
xcode_version=$xcode_version
bundle_id=$bundle_id
bundle_executable=$executable_name
architectures=$architectures
denied_host_node_modules=$denied_host_node_modules
build_output=$out_dir
build_log=$log_path
generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF
cp "$out_dir/args.gn" "$artifact_dir/args.gn"
(
    cd "$artifacts"
    ln -sfn "$revision" current
)

report "iphone_chromium_artifact=$artifact_app"
report "iphone_chromium_bundle_id=$bundle_id"
report "iphone_chromium_architectures=$architectures"
report 'iphone_chromium_source_build=complete'
