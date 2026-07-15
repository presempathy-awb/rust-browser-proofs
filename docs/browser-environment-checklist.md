# Browser And Environment Checklist

Last reviewed: 2026-07-14. This is an evidence ledger, not a compatibility
claim. A checked item has fresh evidence; an unchecked item is either wired but
not executed, or blocked by the prerequisite stated beside it.

## Generic Crate Contract

- [x] `rust-browser-proofs` remains a dev-only test crate with no PageDB dependency.
- [x] A standalone consumer fixture compiles for `wasm32-unknown-unknown`.
- [x] `just test-consumer-battery` executed the fixture's
  `opfs_worker_battery!()` in headless Chrome and Firefox: sync-handle round
  trip and raw sync baseline both passed in each engine.
- [x] `rust-browser-proofs -- <command>` selects Rustup's `rustc` and `cargo`
  for a consumer-selected regular test command.
- [ ] A hosted Git revision has been pinned and exercised by an unrelated
  repository. The current proof is local-only.

## PageDB Integration

- [x] PageDB's `smoke` test consumes the generic sync-handle helper and passed
  in headless Chrome.
- [x] PageDB's `raw_sync_benchmark` consumes the generic raw I/O helper and
  passed both browser tests in headless Chrome.
- [x] Native workspace tests passed.
- [x] Generic-consumer and PageDB harness wasm32 compile checks passed.
- [ ] The full PageDB durable Chrome matrix has not been rerun after the crate
  extraction. It requires the repository-pinned ChromeDriver.

## Browser Matrix

### Desktop Chrome Or Chromium

- [x] Google Chrome is installed on this macOS host.
- [x] The generic consumer and targeted PageDB smoke/raw-sync tests passed via
  `wasm-pack`.
- [x] `browser-proofs/.tools/chromedriver` is present and version-matched at
  `150.0.7871.115`.
  `just install-chrome-driver` obtains it from Chrome for Testing for the
  installed Chrome version; strict Chrome recipes run that bootstrap before
  starting WebDriver.
- [x] `just test-consumer-battery` passes the public consumer battery in both
  installed desktop browser engines.
- [ ] Full PageDB durable suite rerun through `just test-chrome`.

### Desktop Firefox

- [x] Firefox is installed on this macOS host.
- [x] Generic consumer battery executed in Firefox through
  `just test-consumer-battery`.
- [ ] Full PageDB durable suite executed through `just test-firefox`.

### Playwright Chromium

- [x] Container-only Playwright-Core proof executed through
  `just container-test-consumer-playwright`. It launches the image's system
  Chromium headlessly and must not download a Playwright-managed browser. Both
  public OPFS battery tests passed.

### Puppeteer Chromium

- [x] Container-only Puppeteer-Core proof executed through
  `just container-test-consumer-puppeteer`. It launches the image's system
  Chromium headlessly with Chromium's sandbox intact and must not download a
  Puppeteer-managed browser. The locked 80-package tree and image are subject to
  the repository security scans. Both public OPFS battery tests passed.

### Opera

- [x] Deferred deliberately. Opera is Chromium-derived and is not an
  independent engine result; the container does not add its third-party package
  source without an Opera-specific product requirement.

### Desktop Edge

- [x] Microsoft Edge `150.0.4078.65` is installed on this macOS host.
- [x] `just test-consumer-battery-edge` passed the public OPFS battery in an
  isolated headless Edge profile. The recipe drives Edge through its local CDP
  endpoint because the current upstream runner's legacy Edge WebDriver path
  does not complete.

### Desktop Safari Or WebKit

- [x] `/usr/bin/safaridriver` is installed.
- [x] Remote Safari automation session verified with `just check-safari-driver`.
- [x] Generic consumer battery executed in Safari: both the sync-access-handle
  round trip and raw-sync baseline passed.
- [ ] Full PageDB durable suite executed through `just test-safari`.

### Android Chrome

- [x] Android Debug Bridge is installed.
- [x] A local Android emulator is provisioned and can launch Chrome.
- [x] Generic consumer battery executed on Android Chrome through the
  emulator-only CDP route. Automated recipes ignore ambient `ANDROID_SERIAL`
  and accept only `ANDROID_EMULATOR_SERIAL` values beginning with `emulator-`.
  The route patches only the upstream test-runner's unconditional `SharedWorker`
  wrapper, resets the test emulator's Chrome profile for a fresh OPFS quota
  state, and requires the browser-reported success result.
- [ ] PageDB bounded smoke executed through `just test-android-chrome`.
- [ ] Full PageDB Android matrix executed through `just test-android-chrome-all`.

### iPhone Safari

- [x] An iOS simulator was booted (`iPhone 17 Pro`).
- [x] MobileSafari availability verified with `just check-iphone-safari`.
- [x] Generic consumer battery executed in iPhone Safari: both public OPFS
  tests passed through SafariDriver's iOS simulator capability.
- [ ] Full PageDB iPhone Safari suite executed through `just test-iphone-safari`.

### iPhone Chrome

- [x] An iOS simulator is booted.
- [ ] Chrome for iOS is not installed in the simulator. No simulator-compatible
  `com.google.chrome` bundle is available locally.
- [ ] Chrome app-shell launch verified with `just check-iphone-chrome`.
- [ ] This target is an app-shell check only. Chrome for iOS uses WebKit, so it
  does not create an independent browser-engine durability result.

### Edge

- [x] Edge is covered by `just test-consumer-battery-edge`; no EdgeDriver is
  required for the generic battery.

## Toolchain And CI

- [x] Rustup owns the installed `wasm32-unknown-unknown` target.
- [x] The runner and wasm recipes select Rustup rather than the ambient
  Homebrew Rust, whose sysroot lacks that target.
- [x] `cargo fmt --all -- --check` and workspace Clippy passed.
- [x] Gitea workflow YAML parses and includes the generic consumer wasm check.
- [x] Gitea Actions run
  [`2456`](https://git.telpher.stream/awb/rust-browser-proofs/actions/runs/2456)
  passed on `4660191`: it fetched the private PageDB dependency through the
  runner's internal Gitea SSH route, then completed formatting, Clippy, native
  tests, consumer Wasm compilation, and the PageDB Wasm check. The
  `PAGEDB_DEPLOY_KEY` secret is a dedicated read-only deploy key for this
  repository's workflow.

## Containerized Desktop Lane

- [x] The local Docker image was built and run for this revision.
- [x] Desktop Chromium consumer battery executed through
  `just container-test-consumer-chrome`.
- [x] Desktop Firefox consumer battery executed through
  `just container-test-consumer-firefox`.
- [x] Container report generated through `just container-report <path>`.
- [x] Safari/iPhone are intentionally outside the Linux container scope.
- [x] Android Chrome is intentionally outside the default container scope; the
  host-backed AVD route needs no attached device, and Docker Desktop on macOS
  does not provide a reliable nested-virtualization lane.

## Commands

Run any ordinary test command under the Rustup toolchain from this repository:

```sh
mise trust .mise.toml
cd fixtures/consumer-battery
rust-browser-proofs -- wasm-pack test --headless --chrome
```

The checked-in entrypoint delegates to the crate binary, so it follows the
current checkout while preserving the working directory. `wasm-pack` therefore
needs a Cargo package directory; the repository root is a virtual workspace.
The explicit Cargo form works from a sibling package checkout:

```sh
cargo run \
  --manifest-path /Users/andrew/code/pres/brow/rust-browser-proofs/crates/rust-browser-proofs/Cargo.toml \
  -- -- wasm-pack test --headless --chrome
```

An optional global install has the same interface but must be refreshed after
local runner changes:

```sh
cargo install --path crates/rust-browser-proofs
```

Generate a current Markdown host report without treating availability as test
execution:

```sh
rust-browser-proofs --report /tmp/rust-browser-proofs-environment.md
```

To add the outcome of a Chrome test, run it from a Cargo package directory:

```sh
cd fixtures/consumer-battery
rust-browser-proofs \
  --report /tmp/rust-browser-proofs-chrome.md \
  -- wasm-pack test --headless --chrome
```

The generated report records Chrome/Firefox/Safari/Edge host presence,
ChromeDriver, Android Debug Bridge/device state, iOS simulator state, Rustup,
and the wasm target. It marks a desktop browser passed only when that explicit
`wasm-pack` browser command succeeds; all other targets remain unexecuted.

Re-check this document after each browser or device run. Do not mark an item
complete from configuration, binary presence, or another browser's result.
