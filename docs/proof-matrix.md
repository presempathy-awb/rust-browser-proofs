# Browser Proof Matrix

Last audited: 2026-07-16.

This matrix separates two claims that share browser prerequisites but have
different owners and different meanings. A green generic battery does not make
PageDB durable green, and a PageDB harness pass does not itself prove that the
reusable crate works for an unrelated consumer.

## Status Terms

| Status | Meaning |
|---|---|
| **Verified** | The named command passed against the named target, with its date and evidence recorded below. |
| **Partial** | The named command passed every scenario listed in evidence except an explicitly identified browser limitation. This is not a full pass. |
| **Not applicable** | The target would duplicate an already-tested engine and no separate product-shell claim is required. |
| **Recorded** | A prior successful run is recorded, but the current source or evidence format requires a fresh run before treating it as release evidence. |
| **Planned** | A command exists but has not produced a qualifying run. |
| **Blocked** | A required target is unavailable; this is not a pass. |

## Claim Boundary

| Proof surface | Owner | What it proves | What it explicitly does not prove |
|---|---|---|---|
| Generic OPFS battery | `crates/rust-browser-proofs` and `fixtures/consumer-battery` | A consumer-owned dedicated worker can open an OPFS sync access handle, round-trip bytes, then complete the bounded raw write/flush/read baseline. | PageDB VFS semantics, commit publication, reopen recovery, crash behavior, or fallback selection. |
| PageDB OPFS durability harness | `harness/` and the pinned `pagedb` dependency | PageDB's OPFS VFS and database commit/recovery behavior on a concrete browser engine. | That the generic crate can be adopted by arbitrary consumers, Safari/mobile support, or the optional IDB adapter. |
| Local PageDB IDB spike | Feature-gated `harness/tests/idb_*.rs` | Viability and selected local behavior of the unmerged PageDB IDB work. | A production fallback, a resolver choice, or parity with the OPFS default suite. |

## Dogfooding Path

The harness consumes the generic primitive before PageDB participates:
`harness/tests/smoke.rs` calls
`rust_browser_proofs::opfs::assert_sync_access_handle_round_trip`, and
`harness/tests/raw_sync_benchmark.rs` calls the generic raw-sync helper. The
PageDB-specific suites then exercise `Db<OpfsVfs>` independently. In the other
direction, `fixtures/consumer-battery` invokes only the public generic macro
and has no PageDB dependency. This prevents PageDB from being the only user of
an otherwise unexercised test API.

## A. Reusable Runner Proves Itself

The public macro emits exactly two browser tests in a crate's own wasm
integration-test target:
`rust_browser_proofs_sync_access_handle_round_trip` and
`rust_browser_proofs_raw_sync_baseline`. `just test-self` runs the crate-owned
integration test; the fixture repeats the same public API through a separate
package and has no PageDB dependency.

| Target | Command | Evidence | Status | Valid claim |
|---|---|---|---|---|
| Runner, native workspace, Wasm consumer compile, scans | `just verify` | Fresh 2026-07-15 run passed format, Clippy, workspace tests, PageDB Wasm compile, runner execution, and source/config/secret scanning. | **Verified** | The checked-in runner and generic fixture remain internally consistent. |
| Crate self-host, Chrome | `just test-self-chrome` | [fresh Chrome self-test report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784107835609-test-status.md) records the crate-owned `wasm-pack` invocation and exit 0. | **Verified** | The crate package compiles and executes its own public battery in Chrome Wasm. |
| Crate self-host, Firefox | `just test-self-firefox` | [fresh Firefox self-test report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784107846555-test-status.md) records the crate-owned `wasm-pack` invocation and exit 0. | **Verified** | The crate package compiles and executes its own public battery in Firefox Wasm. |
| Consumer fixture, Chrome | `just test-consumer-battery-chrome` | [fresh Chrome report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784107101555-test-status.md) records the explicit feature-gated runner invocation and exit 0. | **Verified** | Both public generic OPFS tests passed in desktop Chrome. |
| Consumer fixture, Firefox | `just test-consumer-battery-firefox` | [fresh Firefox report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784107112540-test-status.md) records the explicit feature-gated runner invocation and exit 0. | **Verified** | Both public generic OPFS tests passed in desktop Firefox. |
| Hosted Gitea revision consumer | Isolated package pinned to `6f922604b33b39af1b02cb9accfdcc2fc3843c69` | [Fresh 2026-07-15 completion report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784118482-skipped-lanes-proof-status.md) records Wasm compilation and both public tests passing in Chrome and Firefox from the hosted dependency. | **Verified** | The crate can be resolved from a concrete Gitea revision and exercised outside this workspace. |
| Extended generic target set | `just test-consumer-battery-all` | [Fresh 2026-07-15 completion report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784118482-skipped-lanes-proof-status.md) records Chrome, Firefox, Edge, desktop Safari, iPhone Safari simulator, and Android Chrome emulator passes. | **Verified** | The generic battery passed in every locally supported host and simulator target. |
| Linux container desktop lane | `just container-test-consumer-desktop` and `just container-verify` | [Fresh 2026-07-15 completion report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784118482-skipped-lanes-proof-status.md) records Chromium, Firefox, Playwright-Core, Puppeteer-Core, isolated workspace checks, and a clean image scan. | **Verified** | The isolated Linux image drives the generic battery without host Rust or browsers; it does not cover Safari or mobile. |
| iPhone Chromium app shell | `just run-iphone-chromium-source 'https://example.com/'` | [Fresh source-app report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784170554115-test-status.md) records exit 0 after the arm64 app opened the URL and survived a 15-second crash window. Revision `216dab31f6c4aaf18abd8e85e7af247a48a8a4be` is retained under `~/.volumes/chromium`; all 663 retained files revalidated against `app-files.sha256`. | **Verified** | This proves the public unbranded Chromium iOS app shell without a real phone. It is not branded Google Chrome and adds no engine or storage claim beyond the separately verified iPhone Safari/WebKit rows. |

## B. PageDB OPFS Durability

`just test-browsers` is the primary desktop PageDB gate. It expands to
`just test-chrome` followed by `just test-firefox`; each command builds the
sacrificial worker driver and executes the following default harness suites.

| Suite | Browser test functions in current source | Durability behavior |
|---|---:|---|
| `smoke` | 1 | Dedicated-worker sync-handle round trip before PageDB participates. |
| `bootstrap` | 2 | Capability preflight and dedicated-worker OPFS setup. |
| `raw_sync_benchmark` | 2 | Raw OPFS baseline; not a PageDB performance claim. |
| `vfs_basic` | 2 | End-to-end `OpfsVfs` commit/reopen and read-only behavior. |
| `conformance` | 18 | PageDB VFS reference semantics on real OPFS. |
| `engine` | 8 | Multi-commit/reopen, lifecycle, reconciliation, GC, page-size, and scratch stress. |
| `manifest` | 13 | A/B manifest crash protocol and namespace recovery. |
| `registry` | 8 | Physical-file handle, close, lock, quota, and range-error behavior. |
| `oracle` | 10 | Real worker termination and injected-fault commit recovery. |
| `receipt_browser` | 1 | Browser/native receipt parity over all legal page sizes and reopen. |
| `idb_spike` | 2 | IDB transaction viability only; it is not an `IdbVfs` selection proof. |
| **Default total** | **67** | Counted from the selected test source files. |

| Target | Command | Evidence | Status | Valid claim |
|---|---|---|---|---|
| Desktop Chrome | `just test-chrome` via `just test-browsers` | Fresh 2026-07-15 run exited 0: all 67 default harness tests passed in headless Chrome. | **Verified** | PageDB OPFS durability passed in Chrome; it does not cover Safari or mobile. |
| Desktop Firefox | `just test-firefox` via `just test-browsers` | Fresh 2026-07-15 run exited 0: all 67 default harness tests passed in headless Firefox. | **Verified** | PageDB OPFS durability passed in Firefox; it does not cover Safari or mobile. |
| Desktop Edge | `just test-edge` | [Fresh 2026-07-16 matrix report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784192174-expanded-browser-matrix-status.md) records all 67 default PageDB tests passing in an isolated headless Edge profile through CDP. | **Verified** | PageDB OPFS durability passed in Edge without EdgeDriver. |
| Desktop Safari/WebKit | `just test-safari` | [Fresh 2026-07-15 report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784111109-remaining-proof-status.md) records all 67 PageDB tests passing in desktop Safari. | **Verified** | PageDB OPFS durability passed in desktop Safari. |
| Android Chrome emulator | `just test-android-chrome-all` | [Fresh 2026-07-15 report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784111109-remaining-proof-status.md) records all 67 PageDB tests passing through the emulator-only CDP route. | **Verified** | PageDB OPFS durability passed in Android Chrome without an attached device. |
| iPhone Safari simulator | `just test-iphone-safari` | [Fresh 2026-07-15 report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784111109-remaining-proof-status.md) records all 67 PageDB tests passing in MobileSafari on the booted simulator. | **Verified** | PageDB OPFS durability passed in the iPhone Safari simulator. |

## C. PageDB IDB Viability

These commands use the local-only PageDB
`codex/idb-safari-worker-termination` workspace based on
`codex/idb-vfs-fallback`.
They are evidence for an optional adapter, not authorization to select IDB as
an automatic fallback. The full suite has 25 scenarios: transaction viability,
store atomicity, 15 VFS cases, four worker-termination cuts, receipt parity,
cross-worker exclusion, and cross-tab Web Locks.

| Target | Command | Evidence | Status | Valid claim |
|---|---|---|---|---|
| Desktop Chrome | `just test-idb-chrome` | [Fresh 2026-07-16 matrix report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784192174-expanded-browser-matrix-status.md) records 25/25 scenarios passing headlessly. | **Verified** | The local IDB adapter passed the full viability model in Chrome. |
| Desktop Firefox | `just test-idb-firefox` | The same report records 25/25 scenarios passing headlessly. | **Verified** | The local IDB adapter passed the full viability model in Firefox. |
| Desktop Edge | `just test-idb-edge` | The same report records 25/25 scenarios passing in an isolated headless Edge profile through CDP. | **Verified** | The local IDB adapter passed the full viability model in Edge. |
| Android Chrome emulator | `just test-idb-android-chrome` | The same report records 25/25 scenarios passing in Chrome on the managed windowless Pixel 8 AVD, including popup-backed cross-tab locking. | **Verified** | The local IDB adapter passed the full viability model in Android Chrome without an attached device. |
| Desktop Safari/WebKit | `just test-idb-safari` | [Fresh crash-fix report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784194256-pagedb-idb-safari-worker-termination.md) records the complete current-source recipe exiting 0 with all 25 scenarios, including the formerly blocked active-transaction termination cut. | **Verified** | The local IDB adapter passed all 25 viability scenarios in desktop Safari. |
| iPhone Safari simulator | `just test-idb-iphone-safari` | The same report combines the prior 24 passing scenarios with the fresh missing cut; all four crash boundaries passed in 1.29 seconds. A later one-shot rerun stalled before loading the already-proven cross-worker suite, so the report keeps that runner caveat separate from scenario coverage. | **Verified** | The local IDB adapter has evidence for all 25 viability scenarios in MobileSafari; the monolithic recipe still has a simulator-runner cleanup risk. |
| iPhone Chromium app shell | WebKit rows above plus `just run-iphone-chromium-source` | The source-built app shell is verified separately, but iOS browser apps use WebKit and do not add an independent storage engine result. | **Verified** | App-shell availability is proven; storage behavior is inherited only from the iPhone WebKit row. |
| Opera | None | Opera is Chromium-derived and no trusted Opera package source is admitted solely to duplicate the engine result. | **Not applicable** | No Opera-branded compatibility claim is made. |

## Reading The Result

- Ask **"does the reusable crate work?"**: read section A. The release gate is
  `just verify` plus an explicit consumer-browser command; no PageDB result is
  needed for that narrow question.
- Ask **"is PageDB durable on this browser?"**: read section B. The target is
  green only after its complete PageDB command passes; a generic, container,
  or another-engine result cannot substitute for it.
- Ask **"can we select IDB as fallback?"**: all supported browser lanes now
  have evidence for the 25-scenario viability model, but this matrix still does
  not authorize automatic selection. The experimental adapter, resolver policy,
  and product fallback decision remain separate gates.

The executable suite descriptions remain in [the README matrix](../README.md#whats-here).
The broader host/device ledger is [the browser environment checklist](browser-environment-checklist.md).
