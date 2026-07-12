# silopal-pagedb-browser-proofs

Browser test harness and crash oracle for [pagedb](https://github.com/NodeDB-Lab/pagedb)'s
OPFS backend — the in-worker synchronous `FileSystemSyncAccessHandle`
implementation whose original `feat/opfs-sync-backend` history is merged on
the author's Gitea `main` branch.

pagedb is an encrypted, portable, embedded page store in pure Rust. This
repo proves its OPFS backend against real browsers: every suite runs inside
a dedicated Web Worker (the only context where OPFS sync access handles
exist) in headless Chromium and Firefox.

`silopal-pagedb-opfs` consumes this repository as a pinned Git submodule.
This suite is independently runnable and owns the test sources, browser
drivers, mobile runners, and durable-browser acceptance criteria. It is not a
runtime dependency and does not expose a generic test framework API.

## Browser durability target matrix

A browser is only "durable green" after the full OPFS browser suite passes in
that browser: conformance, engine, manifest, registry, crash oracle, raw OPFS
baseline, bootstrap, and receipt parity. The local-only IDB fallback suites are
separate proof gates and do not make fallback selection available.

| Target | Command | Status boundary |
|---|---|---|
| Desktop Chrome/Chromium | `just test-chrome` | Automated durable OPFS suite. Uses the repo-pinned `.tools/chromedriver` after `just check-chrome-driver`. |
| Desktop Firefox | `just test-firefox` | Automated durable OPFS suite. `wasm-pack` manages GeckoDriver when needed. |
| Desktop Safari/WebKit | `just test-safari` | Explicit durable OPFS target. Verifies `/usr/bin/safaridriver`, then runs the suite; use `just enable-safari-automation` first if Safari automation is disabled. Do not infer Safari from Chrome or Firefox. |
| Android Chrome | `ANDROID_SERIAL=<serial> just test-android-chrome` | Device-backed OPFS smoke retry. Runs one bounded suite (`bootstrap` by default), lowers emulator/test priority, and stops the AVD on exit unless `keep_emulator=1`. Use `just test-android-chrome-all` for the full durable matrix. |
| iPhone Safari | `just test-iphone-safari` | Simulator-backed WebKit target using `safaridriver` iOS capabilities. Boots `IOS_SIMULATOR_ID` or `iPhone 17 Pro`, verifies MobileSafari, and reuses the SafariDriver automation check before running. |
| iPhone Chrome | `just install-iphone-chrome` / `just run-iphone-chrome` | App-shell target only. Chrome for iOS uses the iOS WebKit engine class, but the Chrome app shell is not proven unless `com.google.chrome` is installed and driven. If missing, set `IPHONE_CHROME_APP_PATH` to a simulator-compatible Chrome app bundle. |

`just test-browsers` intentionally stays Chrome + Firefox because those are the
fully automated local defaults. Use `just test-browsers-all` when Safari
automation is enabled. Mobile recipes create a temporary `harness/webdriver.json`
for the run and remove it on exit; a checked-in `webdriver.json` is treated as a
configuration conflict.

Android emulator retries are intentionally conservative. Start with
`just android-status`, then `just test-android-chrome` for the single-suite
smoke. If the emulator or WebDriver run wedges, `just stop-android-emulator`
drains only the repo's configured AVD (`ANDROID_AVD`, default
`pagedb-api35-play`); set `ANDROID_FORCE_KILL=1` only when the AVD refuses to
exit after the normal `adb emu kill` path.

## What's here

| Suite | Cases | Proves |
|---|---|---|
| `smoke` | 1 | Dedicated-worker raw `FileSystemSyncAccessHandle` write/read/flush/close/remove round trip; proves the browser test vehicle before pagedb participates |
| `bootstrap` | 2 | Shipped capability-preflight module dynamically imports in the browser, creates a dedicated worker, exercises a real OPFS sync access handle without requesting persistence, and rejects accidental non-boolean persistence requests |
| `raw_sync_benchmark` | 2 | Raw dedicated-worker OPFS baseline reports repeated 4 KiB sync-handle write+flush and read work, and rejects invalid workload dimensions; it does not claim PageDB VFS or commit performance |
| `vfs_basic` | 2 | First end-to-end `OpfsVfs` trait and `Db` commit/reopen proofs, including read-only write rejection |
| `conformance` | 18 | 1:1 ports of pagedb's `vfs_memory` reference semantics on real OPFS, incl. rename-while-open and the vectored zero-fill contract |
| `engine` | 8 | `Db<OpfsVfs>` end-to-end: multi-commit KV + ordered scans, full segment lifecycle across reopen, crash-shaped reconcile promotion, tombstone GC, all five page sizes, spill scratch stress |
| `manifest` | 13 | The A/B-slot crash protocol: torn slots, both-corrupt refusal (no data loss), crash-after-slot-write adoption, orphan GC, ID-reuse guard, namespace invariants |
| `registry` | 8 | One sync handle per physical file: dedupe, synchronous close, lock semantics, quota and JS-range error typing |
| `oracle` | 10 | **Real worker termination** at seven commit-phase cuts — a sacrificial worker runs a doomed commit through a parking fault-injection VFS, the test kills it mid-operation and asserts publication-grouped recovery (old-exactly / atomic-either-way / new-exactly) |
| `receipt` | 2 | Native ↔ browser behavior parity: a fixed op-script's BLAKE3 receipt matches MemVfs and OpfsVfs for every legal page size, live and across reopen |
| `idb_spike` | 2 | Dedicated-worker IndexedDB binary transaction viability and explicit-abort atomicity gates for a future fallback adapter; they are not an `IdbVfs` or a production fallback |
| `idb_store` | 1 | Opt-in local PageDB `idb` feature proof: atomically persists one file image and namespace checkpoint in Firefox; it is not an `IdbVfs` or resolver fallback |
| `idb_vfs` | 15 | Opt-in local PageDB `IdbVfs` workflows: all memory/Tokio reference VFS semantics, sync and reopen visibility, browser-real request errors and transaction aborts, injected `QuotaExceededError` → `NoSpace` mapping at file and namespace sync, post-commit orphan cleanup and retry, plus local and browser-wide locks in Firefox and Chromium; it is not a selectable fallback |
| `idb_crash` | 4 | Real browser worker termination before, during, and after `IdbVfs` namespace publication plus after a PageDB header write before its persistence sync: unpublished paths stay hidden and reclaimable, published paths reopen, and the pre-header-sync database recovers exactly the prior commit in Firefox and Chromium |
| `idb_receipt` | 1 | Opt-in local PageDB `IdbVfs` engine receipt parity for every legal page size across a full Firefox and Chromium reopen; it is not a selectable fallback |
| `idb_cross_worker` | 1 | Firefox and Chromium cross-worker writer-lock contention and post-termination release for `IdbVfs`; it is not a selectable fallback |
| `idb_cross_tab` | 1 | Firefox and Chromium same-origin opener/popup proof of the exact `IdbVfs` Web Locks name and fail-fast protocol: a second tab contends, then acquires after the popup closes; `IdbVfs` itself remains worker-only |

The upstream PR description lives at
[`docs/pr/2026-07-opfs-sync-backend.md`](docs/pr/2026-07-opfs-sync-backend.md);
the PRD and implementation plan are under [`docs/`](docs/).

## Running

Requires [mise](https://mise.jdx.dev), Chrome or Chromium (plus a matching
`chromedriver` in `.tools/` — see the justfile note), and Firefox for the
default desktop proof. Safari and mobile targets have additional WebDriver or
device prerequisites listed above.

```sh
just setup          # toolchain, wasm target, hooks
just install-adb    # installs Android platform-tools through Homebrew if adb is missing
just enable-safari-automation # enables Safari WebDriver automation when macOS admin auth is available
just install-iphone-safari # boots/verifies the iPhone simulator Safari
just install-iphone-chrome # installs IPHONE_CHROME_APP_PATH into the booted simulator when Chrome is missing
just check-chrome-driver # fast local ChromeDriver preflight
just check-safari-driver # verifies SafariDriver can create an automation session
just test-chrome    # all suites, headless Chromium
just test-firefox   # default suites, headless Firefox
just test-safari    # all suites, Safari/WebKit when remote automation is enabled
just android-status # cheap Android device/emulator and WebDriver process check
just test-android-chrome # bounded Android Chrome bootstrap smoke; stops emulator on exit
just test-android-chrome engine # bounded retry of one named Android Chrome suite
just test-android-chrome-all # full Android Chrome suite matrix; still stops emulator on exit
just stop-android-emulator # normal cleanup for the configured Android AVD
just test-iphone-safari # all suites, booted iPhone simulator Safari
just run-android-chrome "http://127.0.0.1:8000" # launch Chrome on the selected Android device
just run-iphone-safari "http://127.0.0.1:8000" # launch MobileSafari in the booted simulator
just run-iphone-chrome "http://127.0.0.1:8000" # launch Chrome iOS app shell when installed
just test-idb-chrome # local-only IDB spike, VFS, file-sync crash, receipt, and cross-worker/cross-tab lock proof
just test-idb-firefox # local-only IDB spike, VFS, file-sync crash, receipt, and cross-worker/cross-tab lock proof
just test-native    # native-side tests (codec, receipt reference)
```

## Gitea Actions prerequisite

The `pagedb-opfs smoke` workflow fetches PageDB from a separate private Gitea
repository. Before it can run, add a read-only `PAGEDB_DEPLOY_KEY` repository
Actions secret whose public half is authorized as a deploy key on `awb/pagedb`.
The workflow validates this prerequisite before attempting Rust setup or a git
fetch; the key is not required for normal local development.

## Browser capability preflight

[`harness/js/pagedb-opfs-bootstrap.mjs`](harness/js/pagedb-opfs-bootstrap.mjs)
reports whether the current origin can run PageDB's dedicated-worker OPFS VFS:

```js
import { probeOpfsCapabilities } from "./harness/js/pagedb-opfs-bootstrap.mjs";

const capability = await probeOpfsCapabilities();
if (!capability.opfs.available || !capability.syncAccessHandle.available) {
  // Surface BackendUnavailable; do not select an unrelated fallback.
}
```

The default probe reports `navigator.storage.estimate()` and existing
persistence status, then creates and removes one temporary file inside a
dedicated worker to exercise `createSyncAccessHandle()`. It does not construct
a database, start a PageDB worker runtime, or request persistent storage. Set
`requestPersistence: true` only when the caller is ready to make that browser
permission request.

## Raw OPFS baseline benchmark

[`harness/js/pagedb-opfs-benchmark.mjs`](harness/js/pagedb-opfs-benchmark.mjs)
measures repeated raw `FileSystemSyncAccessHandle` writes (each followed by
`flush()`) and reads inside one dedicated worker:

```js
import { benchmarkRawSyncAccessHandle } from "./harness/js/pagedb-opfs-benchmark.mjs";

const result = await benchmarkRawSyncAccessHandle({
  byteLength: 4096,
  iterations: 3,
});
console.log(result);
```

`byteLength` and `iterations` are caller-selected positive integers; the
example is only a small correctness-scale workload. The result reports byte
counts and elapsed time, but enforces no throughput target. It is a raw OPFS
baseline—not a measurement of PageDB's VFS or database commit path.

`test-chrome` runs the ChromeDriver preflight first, so an OS-level driver
startup failure is reported before the wasm harness is built. The check only
starts a local WebDriver listener and does not modify browser or driver trust
settings.

> **Dependency note:** `harness/Cargo.toml` pins pagedb to the author's
> private Gitea `main` branch, which contains the original OPFS feature
> history. Use a `.cargo/config.toml` `[patch]` only when deliberately testing
> unmerged local PageDB work; ordinary clean checkouts resolve the declared
> Gitea dependency directly.

> **Local IDB spike:** `idb_store`, `idb_vfs`, `idb_receipt`, and
> `idb_cross_worker` and `idb_cross_tab` require the local-only `codex/idb-vfs-fallback` PageDB
> branch and are deliberately excluded from CI; run `just test-idb-chrome` and
> `just test-idb-firefox`.
> None makes fallback selection available.

## How the crash oracle works

The oracle needs a *real* crash — no destructors, no handle closes — at a
precise point inside pagedb's commit protocol. Cancellation or error
injection inside one worker can't produce that, so:

1. The harness lib is also built as a self-contained `no-modules`
   wasm-bindgen bundle (`just build-driver`) and embedded in the test
   binary.
2. The test spawns a sacrificial dedicated worker from a Blob bootstrap,
   ships it the bundle, and asks it to run a doomed commit through a
   `FaultVfs` armed to park (post a beacon, then await a never-resolving
   promise) at the target operation occurrence.
3. On the beacon, the test calls `Worker.terminate()` — abandoning the
   sync access handles mid-operation — then reopens with a bounded retry
   (OPFS locks release asynchronously) and asserts the recovery contract
   for that cut's publication group.
