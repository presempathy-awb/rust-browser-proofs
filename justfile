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

# Browser suites (dedicated-worker OPFS tests). Browsers required locally.
# .tools/chromedriver (gitignored) must match the installed Chrome major
# version; wasm-pack's auto-fetched driver can drift ahead of the browser.
test-chrome:
    cd harness && wasm-pack test --headless --chrome --chromedriver "{{justfile_directory()}}/.tools/chromedriver"

test-firefox:
    cd harness && wasm-pack test --headless --firefox

test-browsers: test-chrome test-firefox

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
