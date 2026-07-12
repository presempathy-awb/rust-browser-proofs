# pagedb-opfs - operational recipes
# Run `just` (no args) to list recipes.

set dotenv-load := true
set shell := ["bash", "-uc"]

default:
    @just --list

# One-time setup: tools, wasm target, git hooks
setup:
    mise install
    rustup target add wasm32-unknown-unknown
    lefthook install

install-adb:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v adb >/dev/null 2>&1; then
        adb version
        exit 0
    fi
    if ! command -v brew >/dev/null 2>&1; then
        echo "adb is missing and Homebrew is unavailable; install Android platform-tools, then rerun." >&2
        exit 1
    fi
    brew install --cask android-platform-tools
    adb version

install-android-emulator: install-adb
    #!/usr/bin/env bash
    set -euo pipefail
    sdk="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
    avd="${ANDROID_AVD:-pagedb-api35-play}"
    image="${ANDROID_SYSTEM_IMAGE:-system-images;android-35;google_apis_playstore;arm64-v8a}"
    platform="${ANDROID_PLATFORM:-platforms;android-35}"
    if ! command -v sdkmanager >/dev/null 2>&1 || ! command -v avdmanager >/dev/null 2>&1; then
        if ! command -v brew >/dev/null 2>&1; then
            echo "Android command-line tools are missing and Homebrew is unavailable." >&2
            exit 1
        fi
        brew install --cask android-commandlinetools
    fi
    if [[ ! -x /opt/homebrew/opt/openjdk/bin/java ]]; then
        if ! command -v brew >/dev/null 2>&1; then
            echo "OpenJDK is missing and Homebrew is unavailable." >&2
            exit 1
        fi
        brew install openjdk
    fi
    export ANDROID_HOME="$sdk" JAVA_HOME=/opt/homebrew/opt/openjdk
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$PATH"
    yes | sdkmanager --licenses >/dev/null || true
    sdkmanager --install "platform-tools" "emulator" "$platform" "$image"
    if ! avdmanager list avd | grep -q "Name: $avd"; then
        echo "no" | avdmanager create avd --force --name "$avd" --package "$image" --device "pixel_8"
    fi
    avdmanager list avd | sed -n "/Name: $avd/,+4p"

install-android-chromedriver: install-android-emulator
    #!/usr/bin/env bash
    set -euo pipefail
    serial="${ANDROID_SERIAL:-}"
    driver="${ANDROID_CHROMEDRIVER:-{{justfile_directory()}}/.tools/chromedriver-android-124}"
    if [[ -x "$driver" ]]; then
        "$driver" --version
        exit 0
    fi
    version="${ANDROID_CHROMEDRIVER_VERSION:-124.0.6367.207}"
    platform="mac-arm64"
    json="$(mktemp "${TMPDIR:-/tmp}/chrome-for-testing.XXXXXX")"
    zip="$(mktemp "${TMPDIR:-/tmp}/chromedriver-android.XXXXXX.zip")"
    tmp="$(mktemp -d "${TMPDIR:-/tmp}/chromedriver-android.XXXXXX")"
    cleanup() { rm -f "$json" "$zip"; rm -rf "$tmp"; }
    trap cleanup EXIT
    curl -fsSL https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json -o "$json"
    url="$(node -e 'const fs=require("fs"); const data=JSON.parse(fs.readFileSync(process.argv[1],"utf8")); const version=process.argv[2]; const platform=process.argv[3]; const found=data.versions.find(v=>v.version===version); const dl=found && found.downloads && found.downloads.chromedriver && found.downloads.chromedriver.find(d=>d.platform===platform); if (!dl) process.exit(2); console.log(dl.url);' "$json" "$version" "$platform")"
    curl -fsSL "$url" -o "$zip"
    unzip -q "$zip" -d "$tmp"
    mkdir -p "$(dirname "$driver")"
    install -m 0755 "$tmp/chromedriver-$platform/chromedriver" "$driver"
    "$driver" --version

boot-android-emulator: install-android-emulator
    #!/usr/bin/env bash
    set -euo pipefail
    sdk="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
    avd="${ANDROID_AVD:-pagedb-api35-play}"
    boot_timeout="${ANDROID_BOOT_TIMEOUT_SECONDS:-120}"
    export ANDROID_HOME="$sdk" JAVA_HOME=/opt/homebrew/opt/openjdk
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$PATH"
    ready_serial="$(adb devices | awk 'NR > 1 && $2 == "device" { print $1; exit }')"
    if [[ -z "$ready_serial" ]]; then
        mkdir -p "{{justfile_directory()}}/.tools"
        nohup nice -n "${ANDROID_EMULATOR_NICE:-15}" emulator -avd "$avd" -no-window -no-audio -no-boot-anim -gpu swiftshader_indirect >"{{justfile_directory()}}/.tools/android-emulator.log" 2>&1 &
    fi
    for _ in $(seq 1 "$boot_timeout"); do
        ready_serial="$(adb devices | awk 'NR > 1 && $2 == "device" { print $1; exit }')"
        if [[ -n "$ready_serial" ]]; then
            booted="$(adb -s "$ready_serial" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r' || true)"
            if [[ "$booted" == "1" ]]; then
                echo "android_serial=$ready_serial"
                exit 0
            fi
        fi
        sleep 1
    done
    echo "Android emulator did not become ready; inspect .tools/android-emulator.log" >&2
    adb devices -l >&2 || true
    exit 1

android-status:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "adb_devices:"
    if command -v adb >/dev/null 2>&1; then
        adb devices -l || true
    else
        echo "adb missing"
    fi
    echo
    echo "android_test_processes:"
    ps ax -o pid=,ppid=,stat=,comm=,args= | grep -E 'qemu-system-aarch64-headless|emulator -avd|wasm-pack test|wasm-bindgen-test-runner|chromedriver-android' | grep -v grep || true

stop-android-emulator:
    #!/usr/bin/env bash
    set -euo pipefail
    avd="${ANDROID_AVD:-pagedb-api35-play}"
    if ! command -v adb >/dev/null 2>&1; then
        echo "adb_missing=true"
        exit 0
    fi
    adb start-server >/dev/null 2>&1 || true
    mapfile -t emulators < <(adb devices | awk 'NR > 1 && $1 ~ /^emulator-/ && $2 == "device" { print $1 }')
    for serial in "${emulators[@]}"; do
        adb -s "$serial" emu kill >/dev/null 2>&1 || true
    done
    for _ in {1..30}; do
        if ! adb devices | awk 'NR > 1 && $1 ~ /^emulator-/ && $2 == "device" { found=1 } END { exit found ? 0 : 1 }'; then
            break
        fi
        sleep 1
    done
    if pgrep -f "qemu-system-aarch64-headless.*-avd $avd" >/dev/null 2>&1; then
        if [[ "${ANDROID_FORCE_KILL:-0}" == "1" ]]; then
            pkill -TERM -f "qemu-system-aarch64-headless.*-avd $avd" || true
            sleep 3
            pkill -KILL -f "qemu-system-aarch64-headless.*-avd $avd" || true
        else
            echo "Android emulator process for $avd is still exiting." >&2
            echo "Retry later, or run ANDROID_FORCE_KILL=1 just stop-android-emulator to kill only that AVD process." >&2
            exit 1
        fi
    fi
    echo "android_emulator_stopped=$avd"

enable-safari-automation:
    #!/usr/bin/env bash
    set -euo pipefail
    log="$(mktemp "${TMPDIR:-/tmp}/pagedb-opfs-safaridriver-enable.XXXXXX")"
    if /usr/bin/safaridriver --enable >"$log" 2>&1; then
        rm -f "$log"
        exit 0
    fi
    cat "$log" >&2
    rm -f "$log"
    echo "Safari automation could not be enabled non-interactively." >&2
    echo "Run this recipe from an interactive terminal with admin auth, then rerun the Safari tests." >&2
    exit 1

install-iphone-safari:
    #!/usr/bin/env bash
    set -euo pipefail
    sim="${IOS_SIMULATOR_ID:-iPhone 17 Pro}"
    if ! xcrun simctl bootstatus booted -b >/dev/null 2>&1; then
        xcrun simctl boot "$sim" >/dev/null 2>&1 || true
        xcrun simctl bootstatus "$sim" -b
    fi
    xcrun simctl get_app_container booted com.apple.mobilesafari >/dev/null
    echo "iphone_safari_simulator=booted"

install-iphone-chrome: install-iphone-safari
    #!/usr/bin/env bash
    set -euo pipefail
    if xcrun simctl get_app_container booted com.google.chrome >/dev/null 2>&1; then
        echo "iphone_chrome_simulator=installed"
        exit 0
    fi
    app_path="${IPHONE_CHROME_APP_PATH:-}"
    if [[ -z "$app_path" ]]; then
        echo "Chrome for iOS is not installed in the booted simulator." >&2
        echo "Set IPHONE_CHROME_APP_PATH to a simulator-compatible Chrome.app, then rerun." >&2
        echo "App Store iOS apps are not installable into Simulator as durable test fixtures." >&2
        exit 1
    fi
    xcrun simctl install booted "$app_path"
    xcrun simctl get_app_container booted com.google.chrome >/dev/null
    echo "iphone_chrome_simulator=installed"

# Browser suites (dedicated-worker OPFS tests). Browsers required locally.
# .tools/chromedriver (gitignored) must match the installed Chrome major
# version; wasm-pack's auto-fetched driver can drift ahead of the browser.
# Generous per-test timeout: suites are fast in isolation but share the
# machine with builds/review jobs; timing out under load is pure flake.
# The crash oracle embeds a second, self-contained wasm bundle (the
# sacrificial worker's driver) built from the harness lib itself.
build-driver:
    cd harness && wasm-pack build --dev --target no-modules --no-typescript --out-dir pkg-driver

# Self-contained IDB worker for the file-sync termination oracle. The normal
# `idb` feature remains module-based so production Web Locks keep their exact
# browser integration; this driver never calls the lock surface.
build-idb-driver:
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo"; wasm-pack build --dev --target no-modules --no-typescript --out-dir pkg-idb-driver --features idb-crash-driver'

check-chrome-driver:
    #!/usr/bin/env bash
    set -euo pipefail
    driver="{{justfile_directory()}}/.tools/chromedriver"
    if [[ ! -x "$driver" ]]; then
        echo "ChromeDriver is missing or not executable: $driver" >&2
        exit 1
    fi
    port=$((20000 + RANDOM % 20000))
    log="$(mktemp "${TMPDIR:-/tmp}/pagedb-opfs-chromedriver.XXXXXX")"
    pid=''
    cleanup() {
        if [[ -n "$pid" ]]; then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
        rm -f "$log"
    }
    trap cleanup EXIT
    "$driver" --port="$port" --verbose >"$log" 2>&1 &
    pid=$!
    for _ in {1..20}; do
        if curl --fail --silent --max-time 1 "http://127.0.0.1:$port/status" >/dev/null 2>&1; then
            exit 0
        fi
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
    done
    echo "ChromeDriver did not open its WebDriver port within two seconds: $driver" >&2
    if xattr -p com.apple.quarantine "$driver" >/dev/null 2>&1; then
        echo "The driver carries macOS quarantine metadata; this is diagnostic only." >&2
    fi
    echo "Inspect host execution policy and resource pressure; do not change trust settings automatically." >&2
    if [[ -s "$log" ]]; then
        cat "$log" >&2
    else
        echo "ChromeDriver produced no startup output." >&2
    fi
    exit 1

check-safari-driver:
    #!/usr/bin/env bash
    set -euo pipefail
    driver="/usr/bin/safaridriver"
    if [[ ! -x "$driver" ]]; then
        echo "SafariDriver is missing or not executable: $driver" >&2
        exit 1
    fi
    port=$((20000 + RANDOM % 20000))
    log="$(mktemp "${TMPDIR:-/tmp}/pagedb-opfs-safaridriver.XXXXXX")"
    pid=''
    session=''
    cleanup() {
        if [[ -n "$session" ]]; then
            curl --silent -X DELETE "http://127.0.0.1:$port/session/$session" >/dev/null 2>&1 || true
        fi
        if [[ -n "$pid" ]]; then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
        rm -f "$log"
    }
    trap cleanup EXIT
    "$driver" -p "$port" >"$log" 2>&1 &
    pid=$!
    for _ in {1..40}; do
        if curl --fail --silent --max-time 1 "http://127.0.0.1:$port/status" >/dev/null 2>&1; then
            break
        fi
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
    done
    response="$(curl --silent --show-error --max-time 5 -X POST "http://127.0.0.1:$port/session" -H 'Content-Type: application/json' -d '{"capabilities":{"alwaysMatch":{"browserName":"safari"}}}' || true)"
    session="$(printf '%s' "$response" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')"
    if [[ -n "$session" ]]; then
        exit 0
    fi
    echo "SafariDriver is installed, but it cannot create an automation session." >&2
    echo "Enable Safari Settings > Developer > Allow Remote Automation, then rerun this check." >&2
    if [[ -n "$response" ]]; then
        printf '%s\n' "$response" >&2
    fi
    if [[ -s "$log" ]]; then
        cat "$log" >&2
    fi
    exit 1

check-android-chrome: boot-android-emulator install-android-chromedriver
    #!/usr/bin/env bash
    set -euo pipefail
    adb start-server >/dev/null
    mapfile -t devices < <(adb devices | awk 'NR > 1 && $2 == "device" { print $1 }')
    serial="${ANDROID_SERIAL:-}"
    if [[ -z "$serial" ]]; then
        if [[ "${#devices[@]}" -ne 1 ]]; then
            echo "Set ANDROID_SERIAL to choose exactly one Android device/emulator; found ${#devices[@]} ready devices." >&2
            adb devices -l >&2
            exit 1
        fi
        serial="${devices[0]}"
    fi
    if ! adb -s "$serial" shell pm path com.android.chrome >/dev/null 2>&1; then
        echo "Android device $serial does not expose com.android.chrome." >&2
        exit 1
    fi
    echo "android_chrome_serial=$serial"

check-iphone-safari: install-iphone-safari
    #!/usr/bin/env bash
    set -euo pipefail
    if ! xcrun simctl get_app_container booted com.apple.mobilesafari >/dev/null 2>&1; then
        echo "No booted iOS simulator with MobileSafari is available." >&2
        exit 1
    fi
    echo "iphone_safari_simulator=booted"

check-iphone-chrome: install-iphone-chrome
    #!/usr/bin/env bash
    set -euo pipefail
    if ! xcrun simctl get_app_container booted com.google.chrome >/dev/null 2>&1; then
        echo "Chrome for iOS (com.google.chrome) is not installed in the booted simulator." >&2
        echo "Safari/WebKit proof covers the engine class, but not the Chrome iOS app shell." >&2
        exit 1
    fi
    echo "iphone_chrome_simulator=booted"

test-chrome: check-chrome-driver build-driver
    cd harness && WASM_BINDGEN_TEST_TIMEOUT=120 wasm-pack test --headless --chrome --chromedriver "{{justfile_directory()}}/.tools/chromedriver"

test-firefox: build-driver
    cd harness && WASM_BINDGEN_TEST_TIMEOUT=120 wasm-pack test --headless --firefox

test-safari: check-safari-driver build-driver
    #!/usr/bin/env bash
    set -euo pipefail
    cd harness
    for suite in bootstrap conformance engine idb_spike manifest oracle raw_sync_benchmark receipt_browser registry smoke vfs_basic; do
        WASM_BINDGEN_TEST_TIMEOUT=300 wasm-pack test --headless --safari --safaridriver /usr/bin/safaridriver --test "$suite"
    done

test-android-chrome suite="bootstrap" keep_emulator="0" wall_timeout="120" test_timeout="90":
    #!/usr/bin/env bash
    set -euo pipefail
    keep_emulator="{{keep_emulator}}"
    wall_timeout="{{wall_timeout}}"
    test_timeout="{{test_timeout}}"
    test_pid=''
    cleanup() {
        trap - EXIT INT TERM
        rm -f "{{justfile_directory()}}/harness/webdriver.json"
        if [[ -n "$test_pid" ]] && kill -0 "$test_pid" 2>/dev/null; then
            kill "$test_pid" 2>/dev/null || true
            sleep 2
            kill -KILL "$test_pid" 2>/dev/null || true
            wait "$test_pid" 2>/dev/null || true
        fi
        if [[ "$keep_emulator" != "1" ]]; then
            (cd "{{justfile_directory()}}" && just stop-android-emulator) >/dev/null 2>&1 || true
        fi
    }
    trap cleanup EXIT
    trap 'cleanup; exit 130' INT
    trap 'cleanup; exit 143' TERM
    just check-android-chrome
    just build-driver
    cd harness
    if [[ -e webdriver.json ]]; then
        echo "Refusing to overwrite harness/webdriver.json; move it aside before running Android Chrome." >&2
        exit 1
    fi
    serial_part=''
    if [[ -n "${ANDROID_SERIAL:-}" ]]; then
        serial_part=', "androidDeviceSerial": "'"$ANDROID_SERIAL"'"'
    fi
    printf '{ "goog:chromeOptions": { "androidPackage": "com.android.chrome"%s } }\n' "$serial_part" > webdriver.json
    driver="${ANDROID_CHROMEDRIVER:-{{justfile_directory()}}/.tools/chromedriver-android-124}"
    suite="{{suite}}"
    cmd=(wasm-pack test --headless --chrome --chromedriver "$driver")
    if [[ "$suite" != "all" ]]; then
        cmd+=(--test "$suite")
    fi
    echo "android_chrome_suite=$suite"
    echo "android_wall_timeout_seconds=$wall_timeout"
    (
        export WASM_BINDGEN_TEST_TIMEOUT="$test_timeout"
        nice -n "${ANDROID_TEST_NICE:-15}" "${cmd[@]}"
    ) &
    test_pid=$!
    for _ in $(seq 1 "$wall_timeout"); do
        if ! kill -0 "$test_pid" 2>/dev/null; then
            set +e
            wait "$test_pid"
            status=$?
            set -e
            test_pid=''
            exit "$status"
        fi
        sleep 1
    done
    echo "Android Chrome wasm run timed out after ${wall_timeout}s; cleaning up emulator/WebDriver state." >&2
    kill "$test_pid" 2>/dev/null || true
    sleep 2
    kill -KILL "$test_pid" 2>/dev/null || true
    set +e
    wait "$test_pid"
    set -e
    test_pid=''
    exit 124

test-android-chrome-all wall_timeout="120" test_timeout="90":
    #!/usr/bin/env bash
    set -euo pipefail
    cleanup() { (cd "{{justfile_directory()}}" && just stop-android-emulator) >/dev/null 2>&1 || true; }
    trap cleanup EXIT
    for suite in bootstrap conformance engine idb_spike manifest oracle raw_sync_benchmark receipt_browser registry smoke vfs_basic; do
        just test-android-chrome "$suite" 1 "{{wall_timeout}}" "{{test_timeout}}"
    done

test-android-chrome-smoke:
    just test-android-chrome bootstrap

test-iphone-safari: check-safari-driver check-iphone-safari build-driver
    #!/usr/bin/env bash
    set -euo pipefail
    cd harness
    if [[ -e webdriver.json ]]; then
        echo "Refusing to overwrite harness/webdriver.json; move it aside before running iPhone Safari." >&2
        exit 1
    fi
    cleanup() { rm -f webdriver.json; }
    trap cleanup EXIT
    printf '{ "browserName": "safari", "platformName": "ios", "safari:useSimulator": true }\n' > webdriver.json
    for suite in bootstrap conformance engine idb_spike manifest oracle raw_sync_benchmark receipt_browser registry smoke vfs_basic; do
        WASM_BINDGEN_TEST_TIMEOUT=300 wasm-pack test --headless --safari --safaridriver /usr/bin/safaridriver --test "$suite"
    done

run-android-chrome url="about:blank": check-android-chrome
    #!/usr/bin/env bash
    set -euo pipefail
    serial="${ANDROID_SERIAL:-}"
    if [[ -z "$serial" ]]; then
        serial="$(adb devices | awk 'NR > 1 && $2 == "device" { print $1; exit }')"
    fi
    adb -s "$serial" shell am start -a android.intent.action.VIEW -d "{{url}}" com.android.chrome

run-iphone-safari url="about:blank": check-iphone-safari
    xcrun simctl openurl booted "{{url}}"

run-iphone-chrome url="about:blank": check-iphone-chrome
    #!/usr/bin/env bash
    set -euo pipefail
    raw="{{url}}"
    case "$raw" in
        https://*) chrome_url="googlechromes://${raw#https://}" ;;
        http://*) chrome_url="googlechrome://${raw#http://}" ;;
        *) chrome_url="$raw" ;;
    esac
    xcrun simctl openurl booted "$chrome_url"

# Local-only PageDB IDB fallback proof. Requires the gitignored Cargo patch
# to the `codex/idb-vfs-fallback` vendor branch; it is intentionally not CI.
test-idb-firefox: build-idb-driver
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --firefox --test idb_spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --firefox --test idb_store --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --firefox --test idb_vfs --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --firefox --test idb_crash --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --firefox --test idb_receipt --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --firefox --test idb_cross_worker --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --firefox --test idb_cross_tab --features idb-vendor-spike'

test-idb-chrome: check-chrome-driver build-idb-driver
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{justfile_directory()}}/.tools/chromedriver" --test idb_spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{justfile_directory()}}/.tools/chromedriver" --test idb_store --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{justfile_directory()}}/.tools/chromedriver" --test idb_vfs --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{justfile_directory()}}/.tools/chromedriver" --test idb_crash --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{justfile_directory()}}/.tools/chromedriver" --test idb_receipt --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{justfile_directory()}}/.tools/chromedriver" --test idb_cross_worker --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{justfile_directory()}}/.tools/chromedriver" --test idb_cross_tab --features idb-vendor-spike'

test-browsers: test-chrome test-firefox

test-browsers-all: test-chrome test-firefox test-safari

# Native-side tests (manifest codec, receipt reference, etc.)
test-native:
    cargo test --workspace

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Compile-check the harness for wasm32 without running browsers
wasm-check:
    cargo check -p pagedb-opfs-harness --target wasm32-unknown-unknown
