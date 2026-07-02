# PR: In-worker synchronous OPFS backend

**Branch:** `feat/opfs-sync-backend` (base: `main` @ `0d78cfe`)
**Target:** NodeDB-Lab/pagedb (submission by repo owner; this document is the PR description)
**Companion test harness:** `pagedb-opfs` (github.com/presempathy-awb/pagedb-opfs) — browser suites, crash oracle, receipt parity

## Motivation

The shipped OPFS backend had never executed: its worker script was loaded
over a postMessage proxy that no test ever ran, and first execution surfaced
five defects immediately (inverted create-new semantics, unhandled promise
rejections in the worker bridge, double-open self-deadlock on `/main.db`,
`std::time` panics on `wasm32-unknown-unknown`, and rename-while-open
failures that break segment publish). Beyond the defects, the proxy design
paid a message-hop plus byte-marshalling cost on every I/O.

This PR replaces it with an **in-worker synchronous backend**: the whole
`Db` lives inside one dedicated Web Worker (the only context where
`FileSystemSyncAccessHandle` exists), construction is async, and every data
operation afterwards is a direct synchronous sync-access-handle call.

## Architecture

Three layers under the unchanged `Vfs`/`VfsFile` traits:

- **`registry`** — one sync access handle per physical file, refcounted
  (`Arc` strong count), closed synchronously on last drop. The engine
  legitimately double-opens `/main.db` (pager cache + `commit_header`);
  OPFS handle locks are exclusive, so handles must be shared, not reopened.
- **`manifest`** — full logical-path → physical-ID indirection (the
  `opfs-sahpool` pattern). Physical files are named `{id:016x}` and never
  move; `rename` is a manifest entry update, which makes rename-while-open
  work structurally. The manifest itself is a tiny crash-critical store
  with the same discipline as pagedb's header: A/B slot files, each commit
  writes the other slot with `seq+1` plus a truncated-SHA-256 checksum,
  recovery picks the highest valid seq. `sync_dir` commits the manifest —
  it is now a real durability point, not a no-op. Load-time GC collects
  orphaned physical files (skipping registry-live handles), refuses to load
  when both slots are corrupt (distinguishing corrupt from empty so a torn
  store is never mistaken for a fresh one), and keeps `next_id` clear of
  GC survivors so IDs are never reused.
- **`OpfsVfs` / `OpfsFile`** — the trait surface. `Vfs::open`'s future is
  made `Send` by wrapping the JS-future chain in `send_wrapper`'s runtime-
  checked `Future` (single-worker execution model). Construction on the
  main thread fails fast with a typed error.

A small `platform_shim` maps `SystemTime`/`process_id()` to web-time and a
constant on `wasm32-unknown-unknown`, where `std` panics.

## Breaking changes

- `OpfsVfs::new()` is now **async** and must run inside a dedicated worker.
  `OpfsVfs::with_root(dir)` scopes a database to an OPFS subdirectory.
- `OPFS_WORKER_JS`, `protocol.rs`, and `opfs_worker.js` are removed.
- `sync_dir` on this backend commits the manifest (was: no-op). Engine
  call sites are unchanged — pagedb already called `sync_dir` at exactly
  the right points.

## Contract carveout: vectored writes (OPFS only)

`write_at_vectored` pre-extends the file to the batch's max end, then
writes requests sequentially. **On a crash or runtime I/O error mid-batch,
a prefix of the batch may already be durable.** This is the one narrowed
contract point (the trait's all-or-nothing wording is kept for invalid
input, which is still rejected before any byte is written). Commit
atomicity is unaffected: pages only become reachable when the header flip
publishes them. Evidence: the crash oracle's `mid-vectored-write` case
terminates the worker with sub-write 1 durable and the rest unwritten, and
recovery serves exactly the old state in both browsers.

## Post-header metadata error audit

With `sync_dir` carrying real durability work, a swallowed metadata error
is a swallowed durability failure. Every `mkdir_all`/`rename`/`remove`/
`sync_dir` in the post-header paths now either propagates or carries a
written retryability justification:

- `execute_journal_actions` returns `Result`; already-completed renames
  (source missing) are tolerated, everything else propagates, and callers
  keep journal-completion state intact so the next open replays.
- The applyjournal sidecar `sync_dir` propagates (the header commit
  references the sidecar; a dangling journal root would fail every open).
- Promote-side `sync_dir("seg")` propagates in segment swap and
  compaction (post-publication error surfaces; reconcile re-promotes).
- Tombstone-side pairs and reconcile's sweep stay best-effort with
  documented recovery paths (`sweep_orphans` at next open).

## Test evidence

All in the companion harness repo (`pagedb-opfs`), headless Chromium +
Firefox, `wasm_bindgen_test_configure!(run_in_dedicated_worker)`:

- **Conformance (18):** 1:1 ports of `tests/vfs_memory.rs` (including
  `rename_while_open_keeps_handle_alive` and vectored zero-fill) plus
  `vfs_tokio` extras. One documented semantic upgrade: `sync_dir_is_no_op`
  became `sync_dir_commits_namespace`.
- **Engine (8):** multi-commit KV reopen with full readback + ordered
  scans + delete/update, full segment lifecycle across reopen, orphaned
  staging sweep, crash-shaped reconcile promotion (live renamed back to
  staging, reopen promotes), reader-pinned tombstone + `gc_now`, all five
  page sizes, 20-round spill scratch stress.
- **Manifest (12) + registry (8):** A/B crash protocol (torn inactive,
  corrupted active, both-corrupt refusal that preserves data files,
  crash-after-slot-write adoption), orphan GC + ID-reuse guard, namespace
  invariants, handle dedupe/close/locking, quota and JS-range typing.
- **Crash oracle (10):** REAL worker termination at seven commit-phase
  cuts (a sacrificial worker runs a doomed commit through a parking fault
  VFS; the test terminates it mid-operation and reopens). Publication-
  grouped expectations hold in both browsers: pre-publication cuts recover
  exactly the old state; `header-written-pre-sync` is atomic either way,
  never torn; post-publication cuts recover exactly the new state via
  open-flow reconcile. Error-injection cases pin the publication line
  (a `sync_dir` failure after the header flip surfaces to the caller but
  cannot unpublish). Manifest-references-missing-file fails typed.
- **Receipt parity (2):** a fixed op-script's BLAKE3 receipt (ordered
  prefix scan + deleted-key absence + segment pages) is byte-identical
  between MemVfs on native and OpfsVfs in the browser, live and across
  reopen: `42649b29…cf7676`.
- **Native:** `cargo nextest run` 323 passed / 2 skipped (the skips are
  the pre-existing in-process fcntl "cross-process" lock tests, now
  `#[ignore]`d with rationale — fcntl record locks are per-process).
  Note: this PR wires in `tests/durability/`, which was previously not
  compiled by any test target (and had bit-rotted against the current
  API), and adds `tests/durability/metadata_errors.rs`.

## Commit series

`registry` → `manifest` → in-worker `OpfsVfs` rewrite (+ platform shim) →
review-hardening rounds (kept as separate commits for review provenance) →
post-header metadata audit → docs. Each commit passes the native suite.

## CI note

The upstream wasm job (`cargo check --target wasm32-unknown-unknown`) is
untouched and still passes. Browser test CI lives in the harness repo
(`just test-chrome` / `just test-firefox`); wiring a browser job into
upstream CI is offered as a follow-up.

## M2 roadmap (out of scope here)

Web Locks-based multi-tab single-writer arbitration, Mode 2 (async facade
callable from the main thread), a packaged worker bootstrap, benches, and
WebKit coverage — per the PRD's M1/M2 split.
