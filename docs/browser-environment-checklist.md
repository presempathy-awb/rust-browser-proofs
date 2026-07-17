# Browser And Environment Checklist

Last reviewed: 2026-07-16. This is an evidence ledger, not a compatibility
claim. A checked item has fresh evidence; an unchecked item is either wired but
not executed, or blocked by the prerequisite stated beside it.

The [proof matrix](proof-matrix.md) separates the reusable crate's generic
OPFS claim from the PageDB harness durability claim. Do not promote a checked
generic result to PageDB durability coverage.

## Generic Crate Contract

- [x] `rust-browser-proofs` remains a dev-only test crate with no PageDB dependency.
- [x] A standalone consumer fixture compiles for `wasm32-unknown-unknown`.
- [x] `just test-self` executed the crate-owned
  `opfs_worker_battery!()` integration test in headless Chrome and Firefox;
  both generated tests passed in each engine.
- [x] `just test-consumer-battery` executed the fixture's
  `opfs_worker_battery!()` in headless Chrome and Firefox: sync-handle round
  trip and raw sync baseline both passed in each engine.
- [x] `rust-browser-proofs -- <command>` selects Rustup's `rustc` and `cargo`
  for a consumer-selected regular test command.
- [x] An isolated consumer at
  `/Users/andrew/cache/rust-browser-proofs/hosted-consumer-proof` pinned Gitea
  revision `6f922604b33b39af1b02cb9accfdcc2fc3843c69`, compiled it for Wasm, and
  passed the exported public battery in Chrome and Firefox. No repository or
  remote was mutated to produce this proof.

## PageDB Integration

- [x] PageDB's `smoke` test consumes the generic sync-handle helper and passed
  in headless Chrome.
- [x] PageDB's `raw_sync_benchmark` consumes the generic raw I/O helper and
  passed both browser tests in headless Chrome.
- [x] Native workspace tests passed.
- [x] Generic-consumer and PageDB harness wasm32 compile checks passed.
- [x] `just test-browsers` freshly passed the current 67-case PageDB durable
  matrix in desktop Chrome and Firefox. Chrome uses the repository-pinned
  ChromeDriver.

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
- [x] Full current PageDB durable suite passed through `just test-chrome` as
  part of `just test-browsers`.

### Desktop Firefox

- [x] Firefox is installed on this macOS host.
- [x] Generic consumer battery executed in Firefox through
  `just test-consumer-battery`.
- [x] Full current PageDB durable suite passed through `just test-firefox` as
  part of `just test-browsers`.

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
- [x] `just test-edge` passed all 67 PageDB OPFS scenarios.
- [x] `just test-idb-edge` passed all 25 local PageDB IDB scenarios.

### Desktop Safari Or WebKit

- [x] `/usr/bin/safaridriver` is installed.
- [x] Remote Safari automation session verified with `just check-safari-driver`.
- [x] Generic consumer battery executed in Safari: both the sync-access-handle
  round trip and raw-sync baseline passed.
- [x] Full 67-test PageDB durable suite passed through `just test-safari`.
- [x] Desktop Safari has evidence for all 25 IDB scenarios. The PageDB crash
  driver now keeps the active namespace transaction alive with one pending IDB
  request at a time, and all four worker-termination cuts pass serially.
- [x] Safari commands use best-effort foreground restoration. SafariDriver has
  no true headless mode; strict no-focus proof requires a separate macOS VM or
  Apple host. Set `SAFARI_FOCUS_GUARD=0` only to diagnose automation behavior.

### Android Chrome

- [x] Android Debug Bridge is installed.
- [x] A local Android emulator is provisioned and can launch Chrome.
- [x] Generic consumer battery executed on Android Chrome through the
  emulator-only CDP route. Automated recipes ignore ambient `ANDROID_SERIAL`
  and accept only `ANDROID_EMULATOR_SERIAL` values beginning with `emulator-`.
  The route patches only the upstream test-runner's unconditional `SharedWorker`
  wrapper, resets the test emulator's Chrome profile for a fresh OPFS quota
  state, temporarily enables release-Chrome debug flags, and requires the
  browser-reported success result. It restores the previous Chrome command-line
  file and Android `debug_app` state during cleanup.
- [x] PageDB bounded smoke passed through `just test-android-chrome`.
- [x] Full 67-test PageDB Android matrix passed through
  `just test-android-chrome-all`. The PageDB route uses the same emulator-only
  CDP transport, gives every suite a fresh server port, recursively terminates
  the interactive runner, and restores Chrome command-line and Android
  `debug_app` state during cleanup.
- [x] `just test-idb-android-chrome` passed all 25 IDB scenarios on the managed
  Pixel 8 AVD. The AVD runs with `-no-window`; no Android device is attached or
  required.

### iPhone Safari

- [x] An iOS simulator was booted (`iPhone 17 Pro`).
- [x] MobileSafari availability verified with `just check-iphone-safari`.
- [x] Generic consumer battery executed in iPhone Safari: both public OPFS
  tests passed through SafariDriver's iOS simulator capability.
- [x] Full 67-test PageDB iPhone Safari suite passed through
  `just test-iphone-safari`.
- [x] iPhone Safari has evidence for all 25 IDB scenarios. The previously
  blocked active-transaction worker-termination cut passed with the PageDB
  request-pump crash hook. A later one-shot rerun stalled before loading the
  already-proven cross-worker suite, so simulator-runner cleanup remains a
  separate reliability item.

### Optional Local IDB

- [x] The gitignored workspace Cargo patch selects the isolated local PageDB
  `codex/idb-safari-worker-termination` workspace without changing the committed
  dependency.
- [x] `just test-idb-chrome` passed 25 opt-in IDB tests in Chrome.
- [x] `just test-idb-firefox` passed the same 25 opt-in IDB tests in Firefox.
- [x] `just test-idb-edge` passed the same 25 opt-in IDB tests in Edge.
- [x] `just test-idb-android-chrome` passed the same 25 opt-in IDB tests in the
  windowless Android Chrome emulator.
- [x] Desktop and iPhone Safari each have evidence for all 25 scenarios. The
  fixed crash hook keeps a real IDB transaction active with one request at a
  time instead of blocking WebKit in a CPU-bound Wasm loop.
- [x] These results prove the local design and selected parity boundaries only;
  they do not select IDB as an automatic fallback.

### iPhone Chrome

- [x] The Google CI archive access, remote OAuth, evidence-preservation,
  source-build fallback, and branded-Chrome boundary are documented in the
  [iPhone Chrome Simulator Runbook](iphone-chrome-simulator.md).
- [x] Both requested Google identities completed OAuth and were tested against
  Stable, Beta, Dev, and Canary archive paths. Both received a confirmed
  `403 storage.objects.list` IAM denial; no archive was listed or downloaded.
- [x] An iOS simulator is booted.
- [x] The installer recognizes Google Chrome's `com.google.chrome.ios` bundle
  ID and derives custom or source-built Chromium bundle IDs from `Info.plist`.
- [x] Source-built arm64 `Chromium.app` revision
  `216dab31f6c4aaf18abd8e85e7af247a48a8a4be` is retained outside `~/code` and
  installed in the simulator as `org.chromium.ost.chrome.ios.dev`.
- [x] Chromium app-shell URL launch passed through
  `just run-iphone-chromium-source 'https://example.com/'`; the process survived
  the 15-second crash window with no new Chromium crash report.
- [ ] Exact branded Google Chrome remains unverified because neither Google
  identity could read the private Simulator archive and an App Store device
  binary is not a Simulator fixture.
- [x] This target is an app-shell check only. Chrome for iOS uses WebKit, so it
  does not create an independent browser-engine durability result.

### Edge

- [x] Edge is covered by `just test-consumer-battery-edge`; no EdgeDriver is
  required for the generic battery.

## Toolchain And CI

- [x] Rustup owns the installed `wasm32-unknown-unknown` target.
- [x] `just install-wasm32-unknown-unknown` installs that target without
  requiring the rest of repository setup.
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

Host operating-system coverage and blockers are tracked independently in the
[host platform matrix](host-platform-matrix.md). In particular, Ubuntu CI is
compile-only; Windows, Manjaro, and Raspberry Pi browser execution are not
currently green.

- [x] QEMU exposes the `raspi4b` machine and boots a checksum-pinned official
  Raspberry Pi kernel to the required Linux and Pi 4 model markers through
  `just test-raspi4b-model`.
- [x] Raspberry Pi simulation artifacts and QEMU logs stay outside the checkout
  under `~/.volumes` by default.
- [ ] The QEMU board-model smoke is intentionally not promoted to Raspberry Pi
  OS, browser, OPFS, network, or physical-hardware evidence.

## Containerized Desktop Lane

- [x] The local Docker image was built and run for this revision.
- [x] Desktop Chromium consumer battery executed through
  `just container-test-consumer-chrome`; the fresh 2026-07-16 arm64 Debian
  image used Chromium and ChromeDriver 150.0.7871.124 and passed both generic
  OPFS tests.
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
  --features runner \
  -- -- wasm-pack test --headless --chrome
```

An optional global install has the same interface but must be refreshed after
local runner changes:

```sh
cargo install --path crates/rust-browser-proofs --features runner
```

Generate a current Markdown host report without treating availability as test
execution:

```sh
rust-browser-proofs --report
```

To add the outcome of a Chrome test, run it from a Cargo package directory:

```sh
cd fixtures/consumer-battery
rust-browser-proofs \
  --report \
  -- wasm-pack test --headless --chrome
```

The generated report records Chrome/Firefox/Safari/Edge host presence,
ChromeDriver, Android Debug Bridge/device state, iOS simulator state, Rustup,
and the wasm target. It marks a desktop browser passed only when that explicit
`wasm-pack` browser command succeeds; all other targets remain unexecuted.

Re-check this document after each browser or device run. Do not mark an item
complete from configuration, binary presence, or another browser's result.
