# Containerized Desktop Browser Proofs

`Dockerfile` builds a local, unprivileged Debian Trixie Linux image containing Rust
1.95, Rustup's `wasm32-unknown-unknown` target, `wasm-pack`, Node.js, Chromium
with the matching Debian ChromeDriver package, and Firefox ESR. Trixie's glibc is
new enough for the ARM64 `wasm-bindgen-test` runner resolved by `wasm-pack`. It
copies the source into the image at build time, so test execution does not need host
Rust, Mise, Node, a browser, or browser drivers.

The only host prerequisite is a working Docker-compatible engine. The image is
local-only and is tagged `rust-browser-proofs:local` by the provided recipes.

## Commands

```sh
just container-build
just container-check
just container-test-consumer-chrome
just container-test-consumer-firefox
just container-report /tmp/rust-browser-proofs-container.md
just security-source
just security-image
just security
```

`container-report` creates a stopped container, runs the report inside it, and
copies only the requested Markdown file to the host. The report describes the
container environment, not the host environment.

## Security Gates

`just security-source` uses a digest-pinned Trivy container to scan source
dependencies, Docker configuration, and secrets. The checkout is mounted
read-only, the scanner has no Docker socket, and its only writable state is the
named `rust-browser-proofs-trivy-cache` advisory cache.

`just security-image` builds the local image, writes it to a temporary Docker
archive, then scans that archive without mounting the Docker socket. Both gates
fail on high or critical findings. The image gate deliberately ignores unfixed
advisories so it remains an actionable release gate; it does not claim that an
upstream base-image advisory is remediated. The source gate does not ignore
unfixed findings.

`just security` runs both gates. `just verify` adds the source gate to native
format, lint, test, Wasm, and command-runner checks; `just container-verify`
adds the image gate to the container workspace check. After `just setup`,
Lefthook runs source security before commit and both verification gates before
push. The hooks are verification-only and use Lefthook's `--no-stage-fixed`
mode, so a partially staged parent/submodule checkout is never reset merely to
run a read-only command. The Trivy database cache can be removed with:

```sh
docker volume rm rust-browser-proofs-trivy-cache
```

## Docker-Only Invocation

The `just` recipes are optional convenience wrappers. With Docker alone, build
the image and run the generic Chromium battery directly:

```sh
docker build --tag rust-browser-proofs:local --file Dockerfile .
docker run --rm --shm-size=1g rust-browser-proofs:local \
  bash -c 'cd "$RUST_BROWSER_PROOFS_WORKSPACE/fixtures/consumer-battery" && rust-browser-proofs -- wasm-pack test --headless --chrome --chromedriver /usr/bin/chromedriver --test opfs_battery'
```

Replace `--chrome --chromedriver /usr/bin/chromedriver` with `--firefox` for
the Firefox battery. The first Firefox run downloads GeckoDriver through
`wasm-pack` into the container user's cache. No host Rust, Mise, Node, browser,
or browser driver is involved in either command.

## Scope Boundary

| Target | Container status | Reason |
|---|---|---|
| Rust/Wasm compile | Supported | Rustup, wasm target, and wasm-pack are installed in the image. |
| Desktop Chromium | Supported | Debian installs Chromium and its matching ChromeDriver. |
| Desktop Firefox | Supported | Firefox ESR is in the image; `wasm-pack` downloads GeckoDriver into the container cache on first use. |
| Safari/WebKit | Not supported | Safari and SafariDriver require macOS. |
| iPhone Safari/Chrome | Not supported | iOS Simulator and Chrome for iOS require macOS/Xcode. |
| Android Chrome | Not default-supported | Device access and emulator virtualization are separate host/device concerns. |
| Edge | Not supported | No Edge package or named runner is included. |

The image runs as the `browser` user, not root. `container-test-consumer-chrome`
uses a 1 GiB shared-memory allocation because headless Chromium is sensitive to
the small default container `/dev/shm` size.

The image runs the generic `rust-browser-proofs` crate and its consumer fixture from
an internal two-member workspace. It intentionally excludes this repository's PageDB
harness, because that harness depends on a private Git source. The container neither
copies the host's local PageDB patch nor contains SSH keys, known-host entries, or
other credentials. Run PageDB-specific tests through the authenticated native
checkout instead.
