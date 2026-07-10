# pagedb-opfs

Browser test harness and crash oracle for [pagedb](https://github.com/NodeDB-Lab/pagedb)'s
OPFS backend — the in-worker synchronous `FileSystemSyncAccessHandle`
implementation developed on the `feat/opfs-sync-backend` branch.

pagedb is an encrypted, portable, embedded page store in pure Rust. This
repo proves its OPFS backend against real browsers: every suite runs inside
a dedicated Web Worker (the only context where OPFS sync access handles
exist) in headless Chromium and Firefox.

## What's here

| Suite | Cases | Proves |
|---|---|---|
| `smoke` | 1 | Dedicated-worker raw `FileSystemSyncAccessHandle` write/read/flush/close/remove round trip; proves the browser test vehicle before pagedb participates |
| `vfs_basic` | 2 | First end-to-end `OpfsVfs` trait and `Db` commit/reopen proofs, including read-only write rejection |
| `conformance` | 18 | 1:1 ports of pagedb's `vfs_memory` reference semantics on real OPFS, incl. rename-while-open and the vectored zero-fill contract |
| `engine` | 8 | `Db<OpfsVfs>` end-to-end: multi-commit KV + ordered scans, full segment lifecycle across reopen, crash-shaped reconcile promotion, tombstone GC, all five page sizes, spill scratch stress |
| `manifest` | 13 | The A/B-slot crash protocol: torn slots, both-corrupt refusal (no data loss), crash-after-slot-write adoption, orphan GC, ID-reuse guard, namespace invariants |
| `registry` | 8 | One sync handle per physical file: dedupe, synchronous close, lock semantics, quota and JS-range error typing |
| `oracle` | 10 | **Real worker termination** at seven commit-phase cuts — a sacrificial worker runs a doomed commit through a parking fault-injection VFS, the test kills it mid-operation and asserts publication-grouped recovery (old-exactly / atomic-either-way / new-exactly) |
| `receipt` | 2 | Native ↔ browser behavior parity: a fixed op-script's BLAKE3 receipt is byte-identical between MemVfs (native) and OpfsVfs (browser), live and across reopen |
| `idb_spike` | 2 | Dedicated-worker IndexedDB binary transaction viability and explicit-abort atomicity gates for a future fallback adapter; they are not an `IdbVfs` or a production fallback |
| `idb_store` | 1 | Opt-in local PageDB `idb` feature proof: atomically persists one file image and namespace checkpoint in Firefox; it is not an `IdbVfs` or resolver fallback |
| `idb_vfs` | 11 | Opt-in local PageDB `IdbVfs` workflows: all memory/Tokio reference VFS semantics, sync and reopen visibility, real transaction aborts, post-commit orphan cleanup and retry, plus local and browser-wide locks in Firefox; it is not a selectable fallback |
| `idb_crash` | 4 | Real Firefox worker termination before, during, and after `IdbVfs` namespace publication plus after a PageDB header write before its persistence sync: unpublished paths stay hidden and reclaimable, published paths reopen, and the pre-header-sync database recovers exactly the prior commit |
| `idb_receipt` | 1 | Opt-in local PageDB `IdbVfs` engine receipt parity across a full Firefox reopen; it is not a selectable fallback |
| `idb_cross_worker` | 1 | Firefox cross-worker writer-lock contention and post-termination release for `IdbVfs`; it is not a selectable fallback |

The upstream PR description lives at
[`docs/pr/2026-07-opfs-sync-backend.md`](docs/pr/2026-07-opfs-sync-backend.md);
the PRD and implementation plan are under [`docs/`](docs/).

## Running

Requires [mise](https://mise.jdx.dev), Chrome or Chromium (plus a matching
`chromedriver` in `.tools/` — see the justfile note), and Firefox.

```sh
just setup          # toolchain, wasm target, hooks
just check-chrome-driver # fast local ChromeDriver preflight
just test-chrome    # all suites, headless Chromium
just test-firefox   # default suites, headless Firefox
just test-idb-firefox # local-only IDB spike, VFS, file-sync crash, receipt, and cross-worker lock proof
just test-native    # native-side tests (codec, receipt reference)
```

`test-chrome` runs the ChromeDriver preflight first, so an OS-level driver
startup failure is reported before the wasm harness is built. The check only
starts a local WebDriver listener and does not modify browser or driver trust
settings.

> **Dependency note:** `harness/Cargo.toml` pins pagedb to the
> `feat/opfs-sync-backend` branch on the author's private remote. Until that
> branch lands upstream, point the `pagedb` git dependency (or a
> `.cargo/config.toml` `[patch]`) at your own checkout of the branch.

> **Local IDB spike:** `idb_store`, `idb_vfs`, `idb_receipt`, and
> `idb_cross_worker` require the local-only `codex/idb-vfs-fallback` PageDB
> branch and are deliberately excluded from CI; run `just test-idb-firefox`.
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
