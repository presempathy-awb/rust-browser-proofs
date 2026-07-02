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
| `conformance` | 18 | 1:1 ports of pagedb's `vfs_memory` reference semantics on real OPFS, incl. rename-while-open and the vectored zero-fill contract |
| `engine` | 8 | `Db<OpfsVfs>` end-to-end: multi-commit KV + ordered scans, full segment lifecycle across reopen, crash-shaped reconcile promotion, tombstone GC, all five page sizes, spill scratch stress |
| `manifest` | 12 | The A/B-slot crash protocol: torn slots, both-corrupt refusal (no data loss), crash-after-slot-write adoption, orphan GC, ID-reuse guard, namespace invariants |
| `registry` | 8 | One sync handle per physical file: dedupe, synchronous close, lock semantics, quota and JS-range error typing |
| `oracle` | 10 | **Real worker termination** at seven commit-phase cuts — a sacrificial worker runs a doomed commit through a parking fault-injection VFS, the test kills it mid-operation and asserts publication-grouped recovery (old-exactly / atomic-either-way / new-exactly) |
| `receipt` | 2 | Native ↔ browser behavior parity: a fixed op-script's BLAKE3 receipt is byte-identical between MemVfs (native) and OpfsVfs (browser), live and across reopen |

The upstream PR description lives at
[`docs/pr/2026-07-opfs-sync-backend.md`](docs/pr/2026-07-opfs-sync-backend.md);
the PRD and implementation plan are under [`docs/`](docs/).

## Running

Requires [mise](https://mise.jdx.dev), Chrome or Chromium (plus a matching
`chromedriver` in `.tools/` — see the justfile note), and Firefox.

```sh
just setup          # toolchain, wasm target, hooks
just test-chrome    # all suites, headless Chromium
just test-firefox   # all suites, headless Firefox
just test-native    # native-side tests (codec, receipt reference)
```

> **Dependency note:** `harness/Cargo.toml` pins pagedb to the
> `feat/opfs-sync-backend` branch on a private Gitea remote. Until that
> branch lands upstream, point the `pagedb` git dependency (or a
> `.cargo/config.toml` `[patch]`) at your own checkout of the branch.

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
