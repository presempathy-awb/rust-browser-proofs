# rust-browser-proofs - generic test-kit checks and PageDB OPFS suite recipes
# Run `just` (no args) to list recipes.

set dotenv-load
set shell := ["bash", "-uc"]

container_image := "rust-browser-proofs:local"
# Trivy v0.70.0 multi-architecture manifest list. The digest prevents mutable
# tag drift while preserving native scanner images on supported CI architectures.
trivy_image := "aquasec/trivy@sha256:be1190afcb28352bfddc4ddeb71470835d16462af68d310f9f4bca710961a41e"
trivy_cache_volume := "rust-browser-proofs-trivy-cache"

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
        echo "Android command-line tools are missing; install the Android SDK command-line tools, then rerun." >&2
        exit 1
    fi
    mise install java
    java_home="$(mise where java)"
    if [[ ! -x "$java_home/bin/java" ]]; then
        echo "Mise did not provide the project-managed Java runtime: $java_home" >&2
        exit 1
    fi
    export ANDROID_HOME="$sdk" JAVA_HOME="$java_home"
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$PATH"
    yes | sdkmanager --licenses >/dev/null || true
    sdkmanager --install "platform-tools" "emulator" "$platform" "$image"
    if ! avdmanager list avd | grep -q "Name: $avd"; then
        echo "no" | avdmanager create avd --force --name "$avd" --package "$image" --device "pixel_8"
    fi
    avdmanager list avd | sed -n "/Name: $avd/,+4p"

install-android-chromedriver: boot-android-emulator
    #!/usr/bin/env bash
    set -euo pipefail
    serial="${ANDROID_EMULATOR_SERIAL:-}"
    if [[ -n "$serial" && "$serial" != emulator-* ]]; then
        echo "ANDROID_EMULATOR_SERIAL must name an emulator-* serial; physical devices are not supported by automated recipes." >&2
        exit 1
    fi
    if [[ -z "$serial" ]]; then
        serial="$(adb devices | awk 'NR > 1 && $1 ~ /^emulator-/ && $2 == "device" { print $1; exit }')"
    fi
    if [[ -z "$serial" ]]; then
        echo "No ready Android emulator was found." >&2
        exit 1
    fi
    browser_version="$(adb -s "$serial" shell dumpsys package com.android.chrome | sed -n 's/.*versionName=\([^ ]*\).*/\1/p' | head -1 | tr -d '\r')"
    if [[ -z "$browser_version" ]]; then
        echo "Could not determine Android Chrome's version on $serial." >&2
        exit 1
    fi
    browser_major="${browser_version%%.*}"
    driver="${ANDROID_CHROMEDRIVER:-{{ justfile_directory() }}/.tools/chromedriver-android-$browser_major}"
    if [[ -x "$driver" ]]; then
        driver_major="$("$driver" --version | awk '{ print $2 }' | cut -d. -f1)"
        if [[ "$driver_major" == "$browser_major" ]]; then
            "$driver" --version
            exit 0
        fi
    fi
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64) platform='mac-arm64' ;;
        Darwin-x86_64) platform='mac-x64' ;;
        Linux-x86_64) platform='linux64' ;;
        *)
            echo "Unsupported Android ChromeDriver platform: $(uname -s)-$(uname -m)" >&2
            exit 1
            ;;
    esac
    json="$(mktemp "${TMPDIR:-/tmp}/chrome-for-testing.XXXXXX")"
    zip="$(mktemp "${TMPDIR:-/tmp}/chromedriver-android.XXXXXX.zip")"
    tmp="$(mktemp -d "${TMPDIR:-/tmp}/chromedriver-android.XXXXXX")"
    cleanup() { rm -f "$json" "$zip"; rm -rf "$tmp"; }
    trap cleanup EXIT
    curl -fsSL https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json -o "$json"
    url="$(node -e 'const fs=require("fs"); const data=JSON.parse(fs.readFileSync(process.argv[1],"utf8")); const major=process.argv[2]; const platform=process.argv[3]; const candidates=data.versions.filter(v=>v.version.startsWith(`${major}.`)); const found=candidates.at(-1); const dl=found?.downloads?.chromedriver?.find(d=>d.platform===platform); if (!dl) process.exit(2); console.log(dl.url);' "$json" "$browser_major" "$platform")" || {
        echo "No Chrome for Testing driver is available for Android Chrome major $browser_major on $platform." >&2
        exit 1
    }
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
    stable_seconds="${ANDROID_BOOT_STABLE_SECONDS:-5}"
    requested_serial="${ANDROID_EMULATOR_SERIAL:-}"
    if ! [[ "$stable_seconds" =~ ^[0-9]+$ ]] || (( stable_seconds < 1 )); then
        echo "ANDROID_BOOT_STABLE_SECONDS must be a positive integer." >&2
        exit 1
    fi
    if [[ -n "$requested_serial" && "$requested_serial" != emulator-* ]]; then
        echo "ANDROID_EMULATOR_SERIAL must name an emulator-* serial; physical devices are not supported by automated recipes." >&2
        exit 1
    fi
    export ANDROID_HOME="$sdk" JAVA_HOME="$(mise where java)"
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$PATH"
    emulator_serial="$(adb devices | awk -v requested="$requested_serial" 'NR > 1 && $1 ~ /^emulator-/ && (requested == "" || $1 == requested) { print $1; exit }')"
    if [[ -z "$emulator_serial" ]]; then
        mkdir -p "{{ justfile_directory() }}/.tools"
        nohup nice -n "${ANDROID_EMULATOR_NICE:-15}" emulator -avd "$avd" -no-window -no-audio -no-boot-anim -gpu swiftshader_indirect >"{{ justfile_directory() }}/.tools/android-emulator.log" 2>&1 &
    fi
    stable_count=0
    for _ in $(seq 1 "$boot_timeout"); do
        ready_serial="$(adb devices | awk -v requested="$requested_serial" 'NR > 1 && $1 ~ /^emulator-/ && $2 == "device" && (requested == "" || $1 == requested) { print $1; exit }')"
        if [[ -n "$ready_serial" ]]; then
            booted="$(adb -s "$ready_serial" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r' || true)"
            if [[ "$booted" == "1" ]] && adb -s "$ready_serial" shell true >/dev/null 2>&1; then
                stable_count=$((stable_count + 1))
            else
                stable_count=0
            fi
            if (( stable_count >= stable_seconds )); then
                echo "android_emulator_serial=$ready_serial"
                exit 0
            fi
        else
            stable_count=0
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
    emulators=()
    while IFS= read -r serial; do
        [[ -n "$serial" ]] && emulators+=("$serial")
    done < <(adb devices | awk 'NR > 1 && $1 ~ /^emulator-/ && $2 == "device" { print $1 }')
    if (( ${#emulators[@]} )); then
        for serial in "${emulators[@]}"; do
            adb -s "$serial" emu kill >/dev/null 2>&1 || true
        done
    fi
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
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo"; wasm-pack build --dev --target no-modules --no-typescript --out-dir pkg-driver'

# Self-contained IDB worker for the file-sync termination oracle. The normal
# `idb` feature remains module-based so production Web Locks keep their exact
# browser integration; this driver never calls the lock surface.
build-idb-driver:
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo"; wasm-pack build --dev --target no-modules --no-typescript --out-dir pkg-idb-driver --features idb-crash-driver'

install-chrome-driver:
    #!/usr/bin/env bash
    set -euo pipefail
    driver="{{ justfile_directory() }}/.tools/chromedriver"
    chrome=''
    for candidate in "${CHROME_BIN:-}" \
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
        "$(command -v google-chrome 2>/dev/null || true)" \
        "$(command -v chromium 2>/dev/null || true)" \
        "$(command -v chromium-browser 2>/dev/null || true)"; do
        if [[ -n "$candidate" && -x "$candidate" ]]; then
            chrome="$candidate"
            break
        fi
    done
    if [[ -z "$chrome" ]]; then
        echo "Chrome or Chromium is required to install the matching ChromeDriver." >&2
        exit 1
    fi
    browser_version="$("$chrome" --version | awk '{ print $NF }')"
    browser_major="${browser_version%%.*}"
    if [[ -x "$driver" ]]; then
        driver_version="$("$driver" --version | awk '{ print $2 }')"
        if [[ "${driver_version%%.*}" == "$browser_major" ]]; then
            echo "ChromeDriver $driver_version already matches Chrome major $browser_major."
            exit 0
        fi
    fi
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64) platform='mac-arm64' ;;
        Darwin-x86_64) platform='mac-x64' ;;
        Linux-x86_64) platform='linux64' ;;
        *)
            echo "Unsupported ChromeDriver platform: $(uname -s)-$(uname -m)" >&2
            exit 1
            ;;
    esac
    metadata="$(mktemp "${TMPDIR:-/tmp}/chrome-for-testing.XXXXXX.json")"
    archive="$(mktemp "${TMPDIR:-/tmp}/chromedriver.XXXXXX.zip")"
    extract_dir="$(mktemp -d "${TMPDIR:-/tmp}/chromedriver.XXXXXX")"
    cleanup() { rm -f "$metadata" "$archive"; rm -rf "$extract_dir"; }
    trap cleanup EXIT
    curl -fsSL https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json -o "$metadata"
    url="$(node -e 'const fs=require("fs"); const data=JSON.parse(fs.readFileSync(process.argv[1],"utf8")); const version=process.argv[2]; const platform=process.argv[3]; const found=data.versions.find(item=>item.version===version); const download=found?.downloads?.chromedriver?.find(item=>item.platform===platform); if (!download) process.exit(2); console.log(download.url);' "$metadata" "$browser_version" "$platform")" || {
        echo "No Chrome for Testing driver is available for Chrome $browser_version on $platform." >&2
        exit 1
    }
    curl -fsSL "$url" -o "$archive"
    unzip -q "$archive" -d "$extract_dir"
    mkdir -p "$(dirname "$driver")"
    install -m 0755 "$extract_dir/chromedriver-$platform/chromedriver" "$driver"
    "$driver" --version

check-chrome-driver: install-chrome-driver
    #!/usr/bin/env bash
    set -euo pipefail
    driver="{{ justfile_directory() }}/.tools/chromedriver"
    if [[ ! -x "$driver" ]]; then
        echo "ChromeDriver is missing or not executable: $driver" >&2
        exit 1
    fi
    chrome=''
    for candidate in "${CHROME_BIN:-}" \
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
        "$(command -v google-chrome 2>/dev/null || true)" \
        "$(command -v chromium 2>/dev/null || true)" \
        "$(command -v chromium-browser 2>/dev/null || true)"; do
        if [[ -n "$candidate" && -x "$candidate" ]]; then
            chrome="$candidate"
            break
        fi
    done
    if [[ -z "$chrome" ]]; then
        echo "Chrome or Chromium is required to validate the installed ChromeDriver." >&2
        exit 1
    fi
    browser_major="$("$chrome" --version | awk '{ print $NF }' | cut -d. -f1)"
    driver_major="$("$driver" --version | awk '{ print $2 }' | cut -d. -f1)"
    if [[ "$driver_major" != "$browser_major" ]]; then
        echo "ChromeDriver major $driver_major does not match installed Chrome major $browser_major." >&2
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
    serial="${ANDROID_EMULATOR_SERIAL:-}"
    if [[ -n "$serial" && "$serial" != emulator-* ]]; then
        echo "ANDROID_EMULATOR_SERIAL must name an emulator-* serial; physical devices are not supported by automated recipes." >&2
        exit 1
    fi
    if [[ -z "$serial" ]]; then
        serial="$(adb devices | awk 'NR > 1 && $1 ~ /^emulator-/ && $2 == "device" { print $1; exit }')"
    fi
    if [[ -z "$serial" ]]; then
        echo "No ready Android emulator was found." >&2
        adb devices -l >&2
        exit 1
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
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{ justfile_directory() }}/.tools/chromedriver"'

test-firefox: build-driver
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --firefox'

test-safari: check-safari-driver build-driver
    #!/usr/bin/env bash
    set -euo pipefail
    cd harness
    for suite in bootstrap conformance engine idb_spike manifest oracle raw_sync_benchmark receipt_browser registry smoke vfs_basic; do
        toolchain="$(dirname "$(rustup which rustc)")"
        PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=300 wasm-pack test --headless --safari --safaridriver /usr/bin/safaridriver --test "$suite"
    done

test-android-chrome suite="bootstrap" keep_emulator="0" wall_timeout="120" test_timeout="90":
    #!/usr/bin/env bash
    set -euo pipefail
    keep_emulator="{{ keep_emulator }}"
    wall_timeout="{{ wall_timeout }}"
    test_timeout="{{ test_timeout }}"
    test_pid=''
    webdriver=''
    webdriver_created='0'
    serial=''
    reverse_port=''
    cleanup() {
        trap - EXIT INT TERM
        if [[ -n "$serial" && -n "$reverse_port" ]]; then
            adb -s "$serial" reverse --remove "tcp:$reverse_port" >/dev/null 2>&1 || true
        fi
        if [[ "$webdriver_created" == "1" && -n "$webdriver" ]]; then
            rm -f "$webdriver"
        fi
        if [[ -n "$test_pid" ]] && kill -0 "$test_pid" 2>/dev/null; then
            kill "$test_pid" 2>/dev/null || true
            sleep 2
            kill -KILL "$test_pid" 2>/dev/null || true
            wait "$test_pid" 2>/dev/null || true
        fi
        if [[ "$keep_emulator" != "1" ]]; then
            (cd "{{ justfile_directory() }}" && just stop-android-emulator) >/dev/null 2>&1 || true
        fi
    }
    trap cleanup EXIT
    trap 'cleanup; exit 130' INT
    trap 'cleanup; exit 143' TERM
    toolchain="$(dirname "$(rustup which rustc)")"
    export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo"
    just check-android-chrome
    package_dir="${ANDROID_TEST_PACKAGE_DIR:-harness}"
    if [[ "$package_dir" != "harness" && "$package_dir" != "fixtures/consumer-battery" ]]; then
        echo "ANDROID_TEST_PACKAGE_DIR must be harness or fixtures/consumer-battery." >&2
        exit 1
    fi
    webdriver="{{ justfile_directory() }}/$package_dir/webdriver.json"
    if [[ "$package_dir" == "harness" ]]; then
        just build-driver
    fi
    cd "$package_dir"
    if [[ -e webdriver.json ]]; then
        echo "Refusing to overwrite $package_dir/webdriver.json; move it aside before running Android Chrome." >&2
        exit 1
    fi
    webdriver_created='1'
    serial_part=''
    if [[ -n "${ANDROID_EMULATOR_SERIAL:-}" ]]; then
        serial_part=', "androidDeviceSerial": "'"$ANDROID_EMULATOR_SERIAL"'"'
    fi
    printf '{ "goog:chromeOptions": { "androidPackage": "com.android.chrome"%s } }\n' "$serial_part" > webdriver.json
    serial="${ANDROID_EMULATOR_SERIAL:-}"
    if [[ -z "$serial" ]]; then
        serial="$(adb devices | awk 'NR > 1 && $1 ~ /^emulator-/ && $2 == "device" { print $1; exit }')"
    fi
    reverse_port="${ANDROID_WASM_BINDGEN_TEST_PORT:-8000}"
    if ! [[ "$reverse_port" =~ ^[0-9]+$ ]] || (( reverse_port < 1 || reverse_port > 65535 )); then
        echo "ANDROID_WASM_BINDGEN_TEST_PORT must be an integer from 1 through 65535." >&2
        exit 1
    fi
    adb -s "$serial" reverse "tcp:$reverse_port" "tcp:$reverse_port"
    browser_major="$(adb -s "$serial" shell dumpsys package com.android.chrome | sed -n 's/.*versionName=\([^ ]*\).*/\1/p' | head -1 | cut -d. -f1 | tr -d '\r')"
    driver="${ANDROID_CHROMEDRIVER:-{{ justfile_directory() }}/.tools/chromedriver-android-$browser_major}"
    suite="${ANDROID_TEST_SUITE:-{{ suite }}}"
    cmd=(wasm-pack test --headless --chrome --chromedriver "$driver")
    if [[ "$suite" != "all" ]]; then
        cmd+=(--test "$suite")
    fi
    echo "android_chrome_suite=$suite"
    echo "android_wall_timeout_seconds=$wall_timeout"
    (
        export WASM_BINDGEN_TEST_TIMEOUT="$test_timeout"
        export WASM_BINDGEN_TEST_ADDRESS="127.0.0.1:$reverse_port"
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
    cleanup() { (cd "{{ justfile_directory() }}" && just stop-android-emulator) >/dev/null 2>&1 || true; }
    trap cleanup EXIT
    for suite in bootstrap conformance engine idb_spike manifest oracle raw_sync_benchmark receipt_browser registry smoke vfs_basic; do
        just test-android-chrome "$suite" 1 "{{ wall_timeout }}" "{{ test_timeout }}"
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
        toolchain="$(dirname "$(rustup which rustc)")"
        PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=300 wasm-pack test --headless --safari --safaridriver /usr/bin/safaridriver --test "$suite"
    done

run-android-chrome url="about:blank": check-android-chrome
    #!/usr/bin/env bash
    set -euo pipefail
    serial="${ANDROID_EMULATOR_SERIAL:-}"
    if [[ -z "$serial" ]]; then
        serial="$(adb devices | awk 'NR > 1 && $1 ~ /^emulator-/ && $2 == "device" { print $1; exit }')"
    fi
    adb -s "$serial" shell am start -a android.intent.action.VIEW -d "{{ url }}" com.android.chrome

run-iphone-safari url="about:blank": check-iphone-safari
    xcrun simctl openurl booted "{{ url }}"

run-iphone-chrome url="about:blank": check-iphone-chrome
    #!/usr/bin/env bash
    set -euo pipefail
    raw="{{ url }}"
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
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{ justfile_directory() }}/.tools/chromedriver" --test idb_spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{ justfile_directory() }}/.tools/chromedriver" --test idb_store --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{ justfile_directory() }}/.tools/chromedriver" --test idb_vfs --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{ justfile_directory() }}/.tools/chromedriver" --test idb_crash --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{ justfile_directory() }}/.tools/chromedriver" --test idb_receipt --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{ justfile_directory() }}/.tools/chromedriver" --test idb_cross_worker --features idb-vendor-spike'
    cd harness && mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" WASM_BINDGEN_TEST_TIMEOUT=120; wasm-pack test --headless --chrome --chromedriver "{{ justfile_directory() }}/.tools/chromedriver" --test idb_cross_tab --features idb-vendor-spike'

test-browsers: test-chrome test-firefox

test-browsers-all: test-chrome test-firefox test-safari

# Consumer compile proof: this fixture only depends on the generic test crate
# and emits its own test battery through the public macro.
check-consumer-battery:
    cargo run -p rust-browser-proofs -- -- cargo check -p rust-browser-proofs-consumer-fixture --target wasm32-unknown-unknown --tests

test-consumer-battery-chrome: check-chrome-driver
    cd fixtures/consumer-battery && cargo run --manifest-path ../../crates/rust-browser-proofs/Cargo.toml -- -- wasm-pack test --headless --chrome --chromedriver "{{ justfile_directory() }}/.tools/chromedriver"

test-consumer-battery-firefox:
    cd fixtures/consumer-battery && cargo run --manifest-path ../../crates/rust-browser-proofs/Cargo.toml -- -- wasm-pack test --headless --firefox

test-consumer-battery: test-consumer-battery-chrome test-consumer-battery-firefox

test-consumer-battery-safari: check-safari-driver
    cd fixtures/consumer-battery && cargo run --manifest-path ../../crates/rust-browser-proofs/Cargo.toml -- -- wasm-pack test --headless --safari --safaridriver /usr/bin/safaridriver --test opfs_battery

test-consumer-battery-iphone-safari: check-safari-driver check-iphone-safari
    #!/usr/bin/env bash
    set -euo pipefail
    cd fixtures/consumer-battery
    if [[ -e webdriver.json ]]; then
        echo "Refusing to overwrite fixtures/consumer-battery/webdriver.json; move it aside before running iPhone Safari." >&2
        exit 1
    fi
    cleanup() { rm -f webdriver.json; }
    trap cleanup EXIT
    printf '{ "browserName": "safari", "platformName": "ios", "safari:useSimulator": true }\n' > webdriver.json
    cargo run --manifest-path ../../crates/rust-browser-proofs/Cargo.toml -- -- wasm-pack test --headless --safari --safaridriver /usr/bin/safaridriver --test opfs_battery

test-consumer-battery-android-chrome keep_emulator="0" wall_timeout="120" test_timeout="90":
    #!/usr/bin/env bash
    set -euo pipefail
    keep_emulator="{{ keep_emulator }}"
    wall_timeout="{{ wall_timeout }}"
    test_timeout="{{ test_timeout }}"
    serial=''
    runner_pid=''
    runner_log=''
    reverse_port=''
    cdp_port=''
    chrome_command_line_backup=''
    chrome_command_line_existed='0'
    cleanup() {
        trap - EXIT INT TERM
        if [[ -n "$runner_pid" ]] && kill -0 "$runner_pid" 2>/dev/null; then
            kill "$runner_pid" 2>/dev/null || true
            wait "$runner_pid" 2>/dev/null || true
        fi
        if [[ -n "$serial" && -n "$reverse_port" ]]; then
            adb -s "$serial" reverse --remove "tcp:$reverse_port" >/dev/null 2>&1 || true
        fi
        if [[ -n "$serial" && -n "$cdp_port" ]]; then
            adb -s "$serial" forward --remove "tcp:$cdp_port" >/dev/null 2>&1 || true
            adb -s "$serial" shell am force-stop com.android.chrome >/dev/null 2>&1 || true
            if [[ "$chrome_command_line_existed" == '1' ]]; then
                adb -s "$serial" shell 'cat > /data/local/tmp/chrome-command-line' <"$chrome_command_line_backup" >/dev/null 2>&1 || true
            else
                adb -s "$serial" shell rm -f /data/local/tmp/chrome-command-line >/dev/null 2>&1 || true
            fi
        fi
        [[ -z "$runner_log" ]] || rm -f "$runner_log"
        [[ -z "$chrome_command_line_backup" ]] || rm -f "$chrome_command_line_backup"
        if [[ "$keep_emulator" != '1' ]]; then
            (cd "{{ justfile_directory() }}" && just stop-android-emulator) >/dev/null 2>&1 || true
        fi
    }
    trap cleanup EXIT
    trap 'cleanup; exit 130' INT
    trap 'cleanup; exit 143' TERM
    if ! [[ "$wall_timeout" =~ ^[0-9]+$ ]] || (( wall_timeout < 1 )); then
        echo 'wall_timeout must be a positive integer.' >&2
        exit 1
    fi
    if ! [[ "$test_timeout" =~ ^[0-9]+$ ]] || (( test_timeout < 1 )); then
        echo 'test_timeout must be a positive integer.' >&2
        exit 1
    fi
    just check-android-chrome
    serial="${ANDROID_EMULATOR_SERIAL:-$(adb devices | awk 'NR > 1 && $1 ~ /^emulator-/ && $2 == "device" { print $1; exit }')}"
    if [[ "$serial" != emulator-* ]]; then
        echo 'The generic Android battery is emulator-only and will not alter a physical device.' >&2
        exit 1
    fi
    reverse_port="${ANDROID_WASM_BINDGEN_TEST_PORT:-8000}"
    cdp_port="${ANDROID_CHROME_CDP_PORT:-9222}"
    for port in "$reverse_port" "$cdp_port"; do
        if ! [[ "$port" =~ ^[0-9]+$ ]] || (( port < 1 || port > 65535 )); then
            echo 'ANDROID_WASM_BINDGEN_TEST_PORT and ANDROID_CHROME_CDP_PORT must be ports from 1 through 65535.' >&2
            exit 1
        fi
    done
    chrome_command_line_backup="$(mktemp "${TMPDIR:-/tmp}/rust-browser-proofs-chrome-command-line.XXXXXX")"
    if adb -s "$serial" shell test -e /data/local/tmp/chrome-command-line; then
        adb -s "$serial" exec-out cat /data/local/tmp/chrome-command-line >"$chrome_command_line_backup"
        chrome_command_line_existed='1'
    fi
    adb -s "$serial" shell pm clear com.android.chrome >/dev/null
    printf 'chrome --remote-debugging-port=%s --disable-fre --no-first-run\n' "$cdp_port" | adb -s "$serial" shell 'cat > /data/local/tmp/chrome-command-line'
    adb -s "$serial" shell chmod 755 /data/local/tmp/chrome-command-line
    adb -s "$serial" shell am start -W -n com.android.chrome/com.google.android.apps.chrome.Main -a android.intent.action.VIEW -d about:blank >/dev/null
    adb -s "$serial" forward --remove "tcp:$cdp_port" >/dev/null 2>&1 || true
    adb -s "$serial" forward "tcp:$cdp_port" localabstract:chrome_devtools_remote
    for _ in $(seq 1 "$wall_timeout"); do
        if curl --fail --silent --max-time 1 "http://127.0.0.1:$cdp_port/json/version" >/dev/null 2>&1; then
            break
        fi
        sleep 1
    done
    if ! curl --fail --silent --max-time 1 "http://127.0.0.1:$cdp_port/json/version" >/dev/null 2>&1; then
        echo 'Android Chrome did not expose its DevTools endpoint.' >&2
        exit 1
    fi
    toolchain="$(dirname "$(rustup which rustc)")"
    runner_log="$(mktemp "${TMPDIR:-/tmp}/rust-browser-proofs-android-runner.XXXXXX")"
    (
        cd "{{ justfile_directory() }}/fixtures/consumer-battery"
        PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" \
            NO_HEADLESS=1 WASM_BINDGEN_TEST_ADDRESS="127.0.0.1:$reverse_port" \
            cargo run --manifest-path ../../crates/rust-browser-proofs/Cargo.toml -- \
            wasm-pack test --headless --chrome --test opfs_battery
    ) >"$runner_log" 2>&1 &
    runner_pid=$!
    for _ in $(seq 1 "$wall_timeout"); do
        if curl --fail --silent --max-time 1 "http://127.0.0.1:$reverse_port/" >/dev/null 2>&1; then
            break
        fi
        if ! kill -0 "$runner_pid" 2>/dev/null; then
            cat "$runner_log" >&2
            exit 1
        fi
        sleep 1
    done
    if ! curl --fail --silent --max-time 1 "http://127.0.0.1:$reverse_port/" >/dev/null 2>&1; then
        echo "The wasm-bindgen interactive server did not become ready after ${wall_timeout}s." >&2
        cat "$runner_log" >&2
        exit 1
    fi
    adb -s "$serial" reverse "tcp:$reverse_port" "tcp:$reverse_port"
    set +e
    node "{{ justfile_directory() }}/scripts/cdp-browser-test.mjs" \
        --cdp-url "http://127.0.0.1:$cdp_port" \
        --url "http://127.0.0.1:$reverse_port/" \
        --timeout-seconds "$test_timeout"

test-consumer-battery-edge wall_timeout="120" test_timeout="90":
    #!/usr/bin/env bash
    set -euo pipefail
    wall_timeout="{{ wall_timeout }}"
    test_timeout="{{ test_timeout }}"
    edge="${EDGE_BINARY:-/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge}"
    test_port="${EDGE_WASM_BINDGEN_TEST_PORT:-8001}"
    cdp_port="${EDGE_CDP_PORT:-9223}"
    runner_pid=''
    edge_pid=''
    runner_log=''
    edge_log=''
    profile=''
    cleanup() {
        trap - EXIT INT TERM
        if [[ -n "$runner_pid" ]] && kill -0 "$runner_pid" 2>/dev/null; then
            kill "$runner_pid" 2>/dev/null || true
            wait "$runner_pid" 2>/dev/null || true
        fi
        if [[ -n "$edge_pid" ]] && kill -0 "$edge_pid" 2>/dev/null; then
            kill "$edge_pid" 2>/dev/null || true
            wait "$edge_pid" 2>/dev/null || true
        fi
        [[ -z "$runner_log" ]] || rm -f "$runner_log"
        [[ -z "$edge_log" ]] || rm -f "$edge_log"
        [[ -z "$profile" ]] || rm -rf "$profile"
    }
    trap cleanup EXIT
    trap 'cleanup; exit 130' INT
    trap 'cleanup; exit 143' TERM
    if [[ ! -x "$edge" ]]; then
        echo "Microsoft Edge executable is missing: $edge" >&2
        echo 'Set EDGE_BINARY to a local Microsoft Edge executable.' >&2
        exit 1
    fi
    for value in "$wall_timeout" "$test_timeout" "$test_port" "$cdp_port"; do
        if ! [[ "$value" =~ ^[0-9]+$ ]] || (( value < 1 || value > 65535 )); then
            echo 'Timeouts must be positive integers and ports must be from 1 through 65535.' >&2
            exit 1
        fi
    done
    if [[ "$test_port" == "$cdp_port" ]]; then
        echo 'EDGE_WASM_BINDGEN_TEST_PORT and EDGE_CDP_PORT must differ.' >&2
        exit 1
    fi
    runner_log="$(mktemp "${TMPDIR:-/tmp}/rust-browser-proofs-edge-runner.XXXXXX")"
    edge_log="$(mktemp "${TMPDIR:-/tmp}/rust-browser-proofs-edge.XXXXXX")"
    profile="$(mktemp -d "${TMPDIR:-/tmp}/rust-browser-proofs-edge-profile.XXXXXX")"
    toolchain="$(dirname "$(rustup which rustc)")"
    (
        cd "{{ justfile_directory() }}/fixtures/consumer-battery"
        PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo" \
            NO_HEADLESS=1 WASM_BINDGEN_TEST_ADDRESS="127.0.0.1:$test_port" \
            cargo run --manifest-path ../../crates/rust-browser-proofs/Cargo.toml -- \
            wasm-pack test --headless --chrome --test opfs_battery
    ) >"$runner_log" 2>&1 &
    runner_pid=$!
    for _ in $(seq 1 "$wall_timeout"); do
        if curl --fail --silent --max-time 1 "http://127.0.0.1:$test_port/" >/dev/null 2>&1; then
            break
        fi
        if ! kill -0 "$runner_pid" 2>/dev/null; then
            cat "$runner_log" >&2
            exit 1
        fi
        sleep 1
    done
    if ! curl --fail --silent --max-time 1 "http://127.0.0.1:$test_port/" >/dev/null 2>&1; then
        echo "The wasm-bindgen interactive server did not become ready after ${wall_timeout}s." >&2
        cat "$runner_log" >&2
        exit 1
    fi
    "$edge" --headless=new --disable-gpu --remote-debugging-port="$cdp_port" \
        --user-data-dir="$profile" --no-first-run --no-default-browser-check about:blank >"$edge_log" 2>&1 &
    edge_pid=$!
    for _ in $(seq 1 "$wall_timeout"); do
        if curl --fail --silent --max-time 1 "http://127.0.0.1:$cdp_port/json/version" >/dev/null 2>&1; then
            break
        fi
        if ! kill -0 "$edge_pid" 2>/dev/null; then
            cat "$edge_log" >&2
            exit 1
        fi
        sleep 1
    done
    if ! curl --fail --silent --max-time 1 "http://127.0.0.1:$cdp_port/json/version" >/dev/null 2>&1; then
        echo "Microsoft Edge did not expose its DevTools endpoint after ${wall_timeout}s." >&2
        cat "$edge_log" >&2
        exit 1
    fi
    node "{{ justfile_directory() }}/scripts/cdp-browser-test.mjs" \
        --cdp-url "http://127.0.0.1:$cdp_port" \
        --url "http://127.0.0.1:$test_port/" \
        --timeout-seconds "$test_timeout"
    status=$?
    set -e
    if (( status != 0 )); then
        cat "$runner_log" >&2
        exit "$status"
    fi

test-consumer-battery-webkit: test-consumer-battery-safari test-consumer-battery-iphone-safari

test-consumer-battery-all: test-consumer-battery test-consumer-battery-edge test-consumer-battery-webkit test-consumer-battery-android-chrome

check-command-runner:
    cargo run -p rust-browser-proofs -- -- /usr/bin/env true

report-environment report_path="/tmp/rust-browser-proofs-environment.md":
    cargo run -p rust-browser-proofs -- --report "{{ report_path }}"

container-build:
    docker build --tag "{{ container_image }}" --file Dockerfile .

container-check: container-build
    docker run --rm "{{ container_image }}" bash -c 'cd "$RUST_BROWSER_PROOFS_WORKSPACE" && cargo test --workspace && cargo check -p rust-browser-proofs-consumer-fixture --target wasm32-unknown-unknown --tests'

container-test-consumer-chrome: container-build
    docker run --rm --shm-size=1g "{{ container_image }}" bash -c 'cd "$RUST_BROWSER_PROOFS_WORKSPACE/fixtures/consumer-battery" && rust-browser-proofs -- wasm-pack test --headless --chrome --chromedriver /usr/bin/chromedriver --test opfs_battery'

container-test-consumer-firefox: container-build
    docker run --rm --shm-size=1g "{{ container_image }}" bash -c 'cd "$RUST_BROWSER_PROOFS_WORKSPACE/fixtures/consumer-battery" && rust-browser-proofs -- wasm-pack test --headless --firefox --test opfs_battery'

container-test-consumer-playwright: container-build
    docker run --rm --shm-size=1g "{{ container_image }}" bash /opt/rust-browser-proofs/playwright/run-opfs-battery.sh

container-test-consumer-puppeteer: container-build
    docker run --rm --init --cap-add=SYS_ADMIN --shm-size=1g "{{ container_image }}" bash /opt/rust-browser-proofs/puppeteer/run-opfs-battery.sh

container-test-consumer-desktop: container-test-consumer-chrome container-test-consumer-firefox container-test-consumer-playwright container-test-consumer-puppeteer

container-report report_path="/tmp/rust-browser-proofs-container-environment.md": container-build
    #!/usr/bin/env bash
    set -euo pipefail
    name="rust-browser-proofs-report-$$"
    trap 'docker rm -f "$name" >/dev/null 2>&1 || true' EXIT
    docker create --name "$name" "{{ container_image }}" bash -c 'rust-browser-proofs --report /tmp/environment.md' >/dev/null
    docker start -a "$name"
    mkdir -p "$(dirname "{{ report_path }}")"
    docker cp "$name:/tmp/environment.md" "{{ report_path }}"
    echo "rust-browser-proofs: copied container report to {{ report_path }}"

# Scan the source tree without giving the scanner write access to this checkout
# or access to the Docker socket. The cache volume holds only Trivy databases.
security-source:
    docker run --rm --read-only --cap-drop ALL --security-opt no-new-privileges:true --tmpfs /tmp:rw,noexec,nosuid,size=512m --mount type=volume,source="{{ trivy_cache_volume }}",target=/root/.cache --mount type=bind,source="{{ justfile_directory() }}",target=/workspace,readonly "{{ trivy_image }}" fs --scanners vuln,misconfig,secret --severity HIGH,CRITICAL --exit-code 1 --skip-dirs .git --skip-dirs target --skip-dirs .tools --skip-version-check /workspace

# Save the local image first so the scanner needs neither Docker's socket nor
# daemon privileges. Ignore unfixed findings in this blocking gate; the report
# remains actionable when the base image cannot yet remediate an advisory.
security-image: container-build
    #!/usr/bin/env bash
    set -euo pipefail
    scan_dir="$(mktemp -d "${TMPDIR:-/tmp}/rust-browser-proofs-scan.XXXXXX")"
    trap 'rm -rf "$scan_dir"' EXIT
    docker save --output "$scan_dir/image.tar" "{{ container_image }}"
    docker run --rm --read-only --cap-drop ALL --security-opt no-new-privileges:true --tmpfs /tmp:rw,noexec,nosuid,size=512m --mount type=volume,source="{{ trivy_cache_volume }}",target=/root/.cache --mount type=bind,source="$scan_dir",target=/scan,readonly "{{ trivy_image }}" image --input /scan/image.tar --scanners vuln,secret --severity HIGH,CRITICAL --ignore-unfixed --exit-code 1 --skip-version-check

# Complete supply-chain gate: source/config/secrets plus the built runtime image.
security: security-source security-image

# Native integrity gate used by Mise and the pre-push hook. Browser execution is
# intentionally a separate, explicit proof because it needs real browser drivers.
verify: fmt-check lint test-native wasm-check check-command-runner security-source

# Container integrity gate used by Mise and the pre-push hook.
container-verify: container-check security-image

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
    mise exec -- sh -c 'toolchain="$(dirname "$(rustup which rustc)")"; export PATH="$toolchain:$PATH" RUSTC="$toolchain/rustc" CARGO="$toolchain/cargo"; cargo check -p pagedb-opfs-harness --target wasm32-unknown-unknown'
