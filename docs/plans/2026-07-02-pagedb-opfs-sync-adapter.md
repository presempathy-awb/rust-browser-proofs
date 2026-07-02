# PageDB OPFS Sync Adapter (M1) Implementation Plan

Created: 2026-07-02
Author: awb@presempathy.com
Agent: Claude Code
Status: PENDING
Approved: Yes
Iterations: 0
Worktree: No
Type: Feature

## Summary

**Goal:** PageDB runs durably in the browser: the rewritten in-worker `OpfsVfs` (direct `FileSystemSyncAccessHandle` calls, logical-path-indirection manifest, per-file handle registry) passes the full VFS conformance suite and a kill-at-any-phase crash oracle against real OPFS in headless Chromium and Firefox, with the change series ready as an upstream PR draft on a `feat/opfs-sync-backend` branch. (PRD: `docs/prd/2026-07-02-pagedb-opfs-adapter.md`, milestone M1.)

## Out of Scope

- **M2 items (next plan, per PRD milestone split):** cross-tab Web Locks + the ReadOnly/Observer typed-error mechanism (M1 keeps the existing in-process `LockMap` — single-tab discipline), quota/eviction surfacing (`estimate()`/`persist()` probes), JS bootstrap module, Mode 2 async bridge, benches, WebKit/Safari runner, PR finalization/submission.
- **`wasm64-unknown-unknown`** — deferred (see Deferred Ideas): Tier-3 nightly Rust, experimental wasm-bindgen memory64, no Safari support, and pagedb cfg-gates on `target_arch = "wasm32"` throughout.
- **Pushing to NodeDB-Lab / opening the actual GitHub PR** — Task 9 produces the commit series and PR description; the user submits upstream. (Pushing to the user's OWN Gitea — both repos — IS in scope: Task 1 initial push, Task 9 final state.)
- **SAB/Atomics anything** — PRD follow-up.
- **Snapshot suite in browser** — `src/snapshot/` is `cfg`-gated native-only; browser equivalent (logical export) is M2+.

## Approach

**Chosen:** Rewrite `src/vfs/opfs/` in-place on a `feat/opfs-sync-backend` branch of the user's pagedb fork clone at `~/code/pres/vendor/pagedb` (cloned in Task 1; the reference checkout at `~/src/lodefolio/data/databases/pagedb` stays pristine), replacing the per-op postMessage proxy (`protocol.rs` + `opfs_worker.js`, deleted) with an in-worker sync backend: async `open` resolves OPFS handles, all data ops call `FileSystemSyncAccessHandle` synchronously through a refcounted per-physical-file registry, and every logical path resolves through an A/B-slot crash-safe manifest (full indirection — the SQLite `opfs-sahpool` pattern). Verification lives in a new `pagedb-opfs` git repo: a wasm-bindgen-test harness (`wasm_bindgen_test_configure!(run_in_dedicated_worker)`) running conformance + engine + crash-oracle suites in headless Chromium and Firefox.
**Why:** Zero message hops and zero byte-boxing on the hot path (the PRD's performance mandate), and full indirection is the only design that genuinely satisfies `rename`-while-open (`traits.rs:31-34`) on OPFS, where `move()` is blocked by open sync handles. Cost: a manifest becomes a tiny crash-critical database of its own — Tasks 3 and 7 pay that down with a specified A/B protocol and dedicated oracle cases.

## Context for Implementer

- **The single-realm rule (all tasks):** `OpfsVfs` and everything it owns live in ONE dedicated worker. The `unsafe impl Send/Sync + SendWrapper` pattern (existing `vfs_impl.rs:133-159` doc comment) is sound only under that assumption — never hand Rust values across workers. `Db<OpfsVfs>` is constructed and driven in the same worker via `wasm-bindgen-futures`.
- **Commit ordering is the crash-safety contract (Tasks 3, 4, 7):** dirty pages vectored-write + `sync()` → inactive A/B header write + `sync()` → segment renames + `sync_dir()` (`src/txn/write.rs` — `flush_main` at :716, `commit_header` at :755, `sync_dir` calls at :769-798). On this backend `rename` = manifest entry update and `sync_dir` = manifest commit — so manifest commits must be atomic (A/B slots, seq + checksum, flush ordering: data flush → inactive slot write → slot flush) and recovery = highest-valid-seq slot + orphan-ID GC.
- **Sync access handles are exclusive per file and worker-bound:** `createSyncAccessHandle()` throws on double-open (`NoModificationAllowedError`). The engine double-opens `/main.db` by design (Pager cache at `src/pager/core.rs:696-705` `OpenMode::CreateOrOpen`, plus `commit_header`/`open_header` reopens at `src/pager/header.rs:95,137`) — the registry MUST dedupe to one sync handle per physical file with refcounted `OpfsFile`s and deterministic (synchronous) close at refcount zero. Read-only enforcement stays Rust-side (`read_only` flag, as today).
- **Browser test isolation:** OPFS persists across tests within one browser session. Every harness test creates its DB under a unique root subdirectory and removes it in teardown; never assume a clean origin.
- **Observer-mode retry loop** (`src/pager/core.rs:516`, the only `tokio::time::sleep` in src) is NOT exercised in M1 — no browser test may open `DbMode::Observer`/`ReadOnly` across contexts; tokio's time driver has no reactor under wasm-bindgen-futures.

## Runtime Environment

- **Harness runs (from `/Users/andrew/code/pres/brow/pagedb-opfs`):** `just test-chrome` / `just test-firefox` → `wasm-pack test --headless --chrome|--firefox harness` (Chrome and Firefox installed locally; toolchain via `mise install` + `just setup`).
- **VCS:** pagedb-opfs is jj-colocated (jj-first: `jj new`/`jj describe`/`jj bookmark`; Gitea `origin` per telpher pattern). The pagedb fork clone (`~/code/pres/vendor/pagedb`) stays plain git on `feat/opfs-sync-backend`; every "pagedb checkout" reference in tasks means this clone, never the pristine lodefolio reference.
- **Native pagedb suite (from `~/code/pres/vendor/pagedb`):** `cargo test` (or `cargo nextest run`); wasm compile check: `cargo check --target wasm32-unknown-unknown --lib --features opfs`.
- **CI:** `.gitea/workflows/smoke.yml` (fmt/clippy/native/wasm-check only — browser suites run locally via just; the Gitea runner has no browsers).

## Feature Inventory

Files being replaced in `pagedb/src/vfs/opfs/` (migration accounting — every function mapped):

| Current artifact | Functions/roles | Disposition |
|---|---|---|
| `protocol.rs` (OpfsRequest/OpfsOp/OpfsResponse/OpfsResult/ErrKind) | postMessage wire protocol | **Deleted** (Task 4) — Mode 1 has no message protocol; M2's bridge redesigns one |
| `opfs_worker.js` + `OPFS_WORKER_JS` export | pure-JS worker: open/read/write/flush/getSize/truncate/remove/copy+delete-rename/listDir/mkdirAll/lock maps | **Deleted** (Task 4) — ops reimplemented in Rust in-worker: registry (Task 2), manifest (Task 3), trait wiring (Task 4) |
| `vfs_impl.rs::OpfsVfs::new(worker_url)` / `dispatch()` / registry-of-oneshots | worker spawn + request correlation | **Replaced** (Task 4): `OpfsVfs::new()` async, resolves OPFS root + loads manifest; no dispatch layer |
| `vfs_impl.rs::LockMap`/`OpfsLockHandle` | in-process advisory locks | **Kept as-is** (Task 4 carries over; Web Locks = M2) |
| `vfs_impl.rs` Vfs methods (open/remove/rename/list_dir/mkdir_all/sync_dir/locks) | trait surface | **Rewritten** (Task 4) over manifest + registry; `sync_dir` becomes manifest commit (was no-op) |
| `handle.rs::OpfsFile` (read_at/read_at_vectored/write_at/write_at_vectored/sync/truncate/len/is_empty) + fire-and-forget Drop | per-file ops via dispatch | **Rewritten** (Tasks 2+4): sync calls via registry; Drop decrements refcount synchronously |
| `handle.rs::map_err` | ErrKind→PagedbError | **Replaced** (Task 2): DOMException-name→PagedbError mapping (adds `QuotaExceededError`→`NoSpace`) |
| `mod.rs` non-wasm shim | `Unsupported` on all methods | **Kept** (Task 4 updates docs only) |

## Assumptions

- **[DISCOVERY, Task 1]** Upstream moved past the reference checkout: the fork branch is based on `origin/main` @ `0d78cfe` (19 commits ahead of lodefolio's `db35f1d`), which refactored the plan's cited files: commit ordering now lives in `src/txn/write/commit.rs` (`flush_main` :158, `commit_header` :197, `sync_dir` :211-240 — not `write.rs` :716-798); the opfs module gained `path.rs` (root-scoped VFS — reuse for per-test namespace isolation in Task 5) and `lock.rs` (extracted LockMap — the "keep as-is" disposition applies to it); the apply-journal became a multi-page sidecar (Task 8's audit examines the actual current files, so its file list is indicative, not authoritative). All other plan file:line citations must be re-checked against the fork clone before use — Tasks 2-8 depend on this note.

- `wasm_bindgen_test_configure!(run_in_dedicated_worker)` runs tests inside a dedicated worker where `createSyncAccessHandle` is available — Tasks 1, 2, 5, 6, 7 depend on this; Task 1's smoke test verifies it first. Fallback if wrong: custom JS test runner page (Task 1 scope grows; flag at Task 1 DoD).
- web-sys's `FileSystemSyncAccessHandle` exposes sync `read_with_u8_array`/`write_with_u8_array`(+`_and_options`), `flush`, `truncate_with_f64`, `get_size`, `close` — Tasks 2, 4 depend on this (feature list already in pagedb Cargo.toml:80-93).
- Terminating a worker releases its OPFS sync-handle locks (possibly asynchronously — reopen may need a bounded retry loop) — Task 7 depends on this.
- The browser assigns real quota to the test origin (headless Chrome/Firefox defaults suffice for multi-MB test DBs) — Tasks 5-7 depend on this.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Manifest commit itself torn/corrupted → whole namespace lost | Medium | High | A/B slots with seq + checksum, never overwrite the active slot, recovery picks highest valid seq; Task 3 native codec tests + Task 7 torn-slot oracle cases gate the design |
| `run_in_dedicated_worker` unsupported/broken in installed wasm-bindgen-test | Low | High | Task 1 smoke test proves it before any backend work; fallback documented (custom runner page) |
| Worker-terminate oracle flaky (async lock release, timing) | Medium | Medium | Beacon-pause protocol makes termination deterministic per phase (driver halts awaiting an ack that never comes); bounded retry on reopen absorbs async lock release |
| Deleting `OPFS_WORKER_JS`/`OpfsVfs::new(worker_url)` breaks unknown downstream users | Low | Medium | Pre-1.0, no crates.io release; PR description (Task 9) lists breaking changes explicitly |
| Crash mid-vectored-write leaves partial page batch on disk (trait's all-or-nothing read strictly) | Medium | Medium | PRD option (b) carveout (Task 4): partial pre-header pages are unreachable by design (CoW header flip + AEAD); Task 7 mid-vectored-write kill case proves the accepted residual; carveout documented in trait docs + PR description |

## Goal Verification

### Truths

1. The complete ported VFS conformance suite — including `rename_while_open_keeps_handle_alive` and `vectored_read_zero_fills_past_eof` — passes against real OPFS in headless Chromium AND Firefox with zero skipped cases; the ONE narrowed contract point (vectored-write crash/error atomicity, PRD option (b)) is explicitly documented in trait docs + PR description, never silently diverged.
2. Killing the DB worker at every instrumented commit phase, plus torn-manifest-slot injection, always reopens to an atomically consistent state per the phase's publication group (pre-publication → exactly the prior committed state; publication-ambiguous → exactly one of {prior, new}, never torn; post-publication → exactly the new state), and the committed-state BLAKE3 receipt for the shared op-script equals the natively computed receipt.
3. The `feat/opfs-sync-backend` branch introduces zero native regressions: pagedb's full native test suite passes unchanged, and `cargo check --target wasm32-unknown-unknown --lib --features opfs` stays green.

## Progress Tracking

- [x] Task 1: Repos + harness workspace with dedicated-worker OPFS smoke test
- [x] Task 2: Sync-handle bindings + refcounted FileRegistry
- [x] Task 3: Manifest — logical namespace with A/B crash protocol
- [x] Task 4: OpfsVfs trait rewrite over registry+manifest; delete postMessage proxy
- [x] Task 5: Port VFS conformance suite to the browser harness
- [x] Task 6: Engine suites (smoke/txn/btree/segment/crash/recovery/page-sizes) in browser
- [ ] Task 7: Crash & durability oracle + native↔browser receipt parity
- [x] Task 8: Post-header metadata error-swallowing audit (upstream)
- [ ] Task 9: Upstream PR draft + documentation sync

## Implementation Tasks

### Task 1: Repos + telpher-family meta kit + dedicated-worker OPFS smoke test

**Objective:** Create the two working repos with the telpher-family meta kit and prove the test vehicle before any backend code: jj-colocated Gitea-first `pagedb-opfs` repo (jj is the priority VCS interface here), the pagedb fork clone at `~/code/pres/vendor/pagedb` on its authorized `feat/opfs-sync-backend` branch, and a wasm-bindgen-test harness whose smoke test runs inside a dedicated worker and successfully exercises a raw `FileSystemSyncAccessHandle` in headless Chromium and Firefox.

**Files:**

- Create: `Cargo.toml` (workspace), `harness/Cargo.toml`, `harness/src/lib.rs`
- Create: `harness/tests/smoke.rs`
- Create: `justfile`, `.mise.toml`, `.gitignore`, `agents.toml`, `lefthook.yml`
- Create: `.gitea/workflows/smoke.yml`
- Create: `~/code/pres/vendor/pagedb` (clone from the local reference checkout `~/src/lodefolio/data/databases/pagedb` for speed; `git remote set-url origin https://github.com/NodeDB-Lab/pagedb`, `git remote add gitea ssh://git@git.telpher.stream:2222/awb/pagedb.git`; then `checkout -b feat/opfs-sync-backend`. Stays plain git, NOT jj — upstream PR discipline. The lodefolio checkout is never modified.)

**Key Decisions / Notes:**

- **VCS**: `jj git init --colocate` in pagedb-opfs (jj-first workflow for this repo, matching telpher/gitspen); configure `origin` as the Gitea remote following telpher's pattern (`ssh://git@git.telpher.stream:2222/awb/pagedb-opfs.git` — telpher's origin is the same host/user). GitHub mirror remote deferred until the user asks.
- **pagedb on Gitea too (user request):** the fork clone carries both remotes (`origin` = github.com/NodeDB-Lab/pagedb for upstream PR tracking; `gitea` = `ssh://git@git.telpher.stream:2222/awb/pagedb.git` — set at clone time per the Create line above). In THIS task: create both repos on the Gitea server (via `tea` CLI if configured, else ask the user for the one-time UI step) and push initial state — pagedb `main` + `feat/opfs-sync-backend` to the `gitea` remote, pagedb-opfs via `jj git push`. Pushes to the user's own Gitea are authorized; pushing anywhere near NodeDB-Lab/GitHub is NOT.
- **Meta kit templates (read these, mirror the shape):** `.mise.toml` from `~/code/pres/scaffold/telpher/.mise.toml` (pin: `rust = "1.95"`, `jj = "0.41"`, `just = "1.52.0"`, add `wasm-pack`, `lefthook`; wasm32 target added via a `just setup` recipe running `rustup target add wasm32-unknown-unknown`); `agents.toml` from `~/code/pres/vcs/gitspen/agents.toml` ([meta] project block, telpher-family framing, pagedb-opfs domain — do not copy gitspen domain rules); `justfile` conventions from telpher (`set dotenv-load`, `set shell := ["bash", "-uc"]`, recipe-list default); `.gitea/workflows/smoke.yml` modeled on telpher's `telpher-smoke.yml` (`runs-on: ubuntu-latest`, checkout@v4) running fmt + clippy + native tests + `cargo check --target wasm32-unknown-unknown` — browser suites stay local via `just` (no browsers on the runner; noted in the workflow file).
- **lefthook.yml**: pre-commit = `cargo fmt --check` (workspace); pre-push = `cargo clippy -- -D warnings`. Installed via `lefthook install` in `just setup`.
- Harness crate: `crate-type = ["cdylib", "rlib"]`, dev-deps `wasm-bindgen-test`, `wasm-bindgen-futures`, `web-sys` (FileSystem* features). **Dependency strategy (CI-compatible):** committed `Cargo.toml` declares `pagedb = { git = "ssh://git@git.telpher.stream:2222/awb/pagedb.git", branch = "feat/opfs-sync-backend", features = ["opfs"] }` so Gitea Actions can build; local development overrides it with a gitignored `.cargo/config.toml` `[patch]` pointing at `/Users/andrew/code/pres/vendor/pagedb` (config-level patch, Cargo ≥1.56). Local iteration never waits on a push; CI builds from the Gitea fork.
- `wasm_bindgen_test_configure!(run_in_dedicated_worker);` at harness test roots — the load-bearing enabler (sync handles are dedicated-worker-only).
- Smoke test: `navigator.storage.getDirectory()` → `getFileHandle("smoke", create)` → `createSyncAccessHandle()` → write/read/close/remove round-trip via raw web-sys (no pagedb involvement yet).
- `just test-chrome` / `just test-firefox` wrap `wasm-pack test --headless --chrome|--firefox harness`.
- Initial jj commit in pagedb-opfs (skeleton + existing docs/); pagedb branch checkout only.

**Definition of Done:**

- [ ] Smoke test writes+reads bytes through a real sync access handle inside a dedicated worker and cleans up after itself
- [ ] `jj -R /Users/andrew/code/pres/brow/pagedb-opfs log -r @ --no-pager` works (colocated repo) and the initial commit contains the meta kit (justfile, .mise.toml, agents.toml, lefthook.yml, .gitea/workflows/smoke.yml); `git -C ~/code/pres/vendor/pagedb branch --show-current` prints `feat/opfs-sync-backend`
- [ ] `lefthook install` registered hooks; `mise install` resolves all pinned tools
- [ ] Both repos exist on the Gitea instance with initial pushes (`git -C ~/code/pres/vendor/pagedb remote get-url gitea` and `jj git push` succeed); `.gitea/workflows/smoke.yml` present and well-formed (runner execution best-effort — verified when the runner picks up the push)
- [ ] Verify: `just test-chrome && just test-firefox` (smoke suite green in both)

### Task 2: Sync-handle bindings + refcounted FileRegistry

**Objective:** Build the physical layer of the new backend in the pagedb branch: OPFS root resolution, physical files named by opaque hex IDs in a flat directory, and a per-physical-file registry that hands out refcounted references to ONE `FileSystemSyncAccessHandle` per file with synchronous close at refcount zero. This is what makes the engine's double-open of `/main.db` (`src/pager/core.rs:696-705` cached handle + `src/pager/header.rs:95,137` reopens) work under OPFS exclusive locking.

**Files:**

- Create: `src/vfs/opfs/registry.rs` (in pagedb branch)
- Test: `harness/tests/registry.rs` (in pagedb-opfs repo)

**Key Decisions / Notes:**

- Registry: `HashMap<PhysId, Entry { sah: FileSystemSyncAccessHandle, refcount: u32 }>`; `open_phys(id, create)` is async (Promise chain: `getFileHandle` → `createSyncAccessHandle`) but returns a cheap clone token; all data ops (`read_at`/`write_at`/`flush`/`truncate`/`size`) are synchronous web-sys calls taking `&mut [u8]`/`&[u8]` directly — no `Vec` copies, no `Array.from`.
- Drop of the last reference closes the sync handle **synchronously** (`close()` is sync) — kills the current fire-and-forget close race (`handle.rs:35-46`) by construction.
- Error mapping helper: DOMException name → `PagedbError` (`NotFoundError`→`Io(NotFound)`, `NoModificationAllowedError`→`AlreadyLocked`-class `Io`, `QuotaExceededError`→`PagedbError::NoSpace`, else `Io(Other)` with the message). Never `unwrap` a `JsValue`.
- TDD: write `harness/tests/registry.rs` cases first (they fail to compile/run until registry lands): dedupe (two opens → one underlying handle, writes visible through both), refcounted close (drop one ref: still open; drop last: reopen succeeds immediately), quota-error mapping smoke (truncate to absurd size → typed error, no trap).

**Definition of Done:**

- [ ] Two simultaneous opens of the same physical ID share one sync handle; interleaved reads/writes through both references observe the same bytes
- [ ] After the last reference drops, an immediate reopen of the same ID succeeds deterministically (no async close race)
- [ ] A quota-exceeding operation surfaces a typed `PagedbError` (no wasm trap)
- [ ] Verify: `just test-chrome && just test-firefox` (registry suite green) and `git -C ~/code/pres/vendor/pagedb diff --stat` shows only `src/vfs/opfs/registry.rs` added

### Task 3: Manifest — logical namespace with A/B crash protocol

**Objective:** Implement the logical-path→physical-ID manifest that gives the backend rename-while-open, directory semantics, and a real `sync_dir`. The manifest codec is target-independent pure-Rust (natively unit-tested); the wasm side persists it in two fixed physical slots with an A/B seq+checksum protocol and garbage-collects orphaned physical IDs on load.

**Files:**

- Create: `src/vfs/opfs/manifest.rs` (in pagedb branch)
- Test: `src/vfs/opfs/manifest.rs` (inline `#[cfg(test)]` native unit tests for the codec)
- Test: `harness/tests/manifest.rs` (browser: persist/reload/torn-slot/GC)

**Key Decisions / Notes:**

- Codec (pure, no wasm deps): `ManifestRecord { version: u32, seq: u64, entries: Vec<(String, EntryKind)> }` where `EntryKind = File(PhysId) | Dir`; encode → bytes + trailing checksum: use `sha2`/`hmac` already in pagedb's tree (Cargo.toml:26-27 — truncated SHA-256 digest is plenty; adds zero new deps); decode validates length + checksum and returns typed corruption errors.
- Persistence: physical slot IDs 0 and 1 reserved (`.slot-a`/`.slot-b` conceptually); commit = write inactive slot bytes → `flush()` slot → in-memory flip; load = read both, pick highest valid seq (mirrors pagedb's own A/B header pattern, `src/pager/header.rs:4-45`).
- Logical ops over the in-memory map: `resolve`, `insert(create)`, `remove`, `rename(from,to)` (entry rewrite — POSIX overwrite semantics: existing `to` entry is replaced, its phys ID orphaned), `list_dir` (direct children only, matching `tests/vfs_memory.rs:146-153`), `mkdir_all` (Dir entries, idempotent). Mutations mark dirty; `sync_dir(path)` commits the manifest (single commit covers all pending mutations).
- Orphan GC on load: enumerate physical dir; any ID not referenced by the winning manifest (and not a slot file) is deleted. GC failures are non-fatal (retried next open) but logged.
- Native codec tests: round-trip, checksum rejection on flipped byte, truncated buffer, seq selection (b>a, a-only-valid, both-invalid → typed error).

**Definition of Done:**

- [ ] Native: codec unit tests pass (`cargo test -p pagedb manifest` in the pagedb checkout)
- [ ] Browser: mutate namespace → `sync_dir` → drop everything → reload picks up exactly the committed namespace; a deliberately corrupted (garbage-overwritten) inactive slot never affects recovery, and a corrupted ACTIVE slot recovers to the other slot's seq
- [ ] Browser: an orphaned physical file (created, never committed to manifest) is removed on next load
- [ ] Verify: `cargo test --lib --features opfs codec_tests` (native, in the fork; `vfs::opfs` is feature-gated so the codec needs `--features opfs`) && `just test-chrome && just test-firefox`

### Task 4: OpfsVfs trait rewrite over registry+manifest; delete postMessage proxy

**Objective:** Replace the async proxy implementation with the in-worker sync backend: rewrite `vfs_impl.rs` and `handle.rs` to implement `Vfs`/`VfsFile` (`src/vfs/traits.rs:18,70`) over Tasks 2+3, delete `protocol.rs`, `opfs_worker.js`, and the `OPFS_WORKER_JS` export, and keep the non-wasm `Unsupported` shim and the in-process `LockMap` unchanged. After this task, `Db<OpfsVfs>` opens, commits, and reopens a real database in the browser.

**Files:**

- Modify: `src/vfs/opfs/vfs_impl.rs`, `src/vfs/opfs/handle.rs`, `src/vfs/opfs/mod.rs` (in pagedb branch)
- Delete: `src/vfs/opfs/protocol.rs`, `src/vfs/opfs/opfs_worker.js` (in pagedb branch)
- Test: `harness/tests/vfs_basic.rs`

**Key Decisions / Notes:**

- `OpfsVfs::new()` becomes `pub async fn new() -> Result<Self>` (resolves `navigator.storage.getDirectory()`, ensures physical dir, loads manifest via Task 3). Constructor signature change + `OPFS_WORKER_JS` removal are the two breaking changes — listed in Task 9's PR description.
- `open(path, mode)`: manifest resolve + `OpenMode` semantics per `src/vfs/types.rs:6-18` (`Read`/`ReadWrite` → NotFound if absent; `CreateNew` → AlreadyExists if present; `CreateOrOpen` never truncates); creates allocate a fresh phys ID + manifest insert (committed lazily — a crash before `sync_dir` loses the uncommitted entry, which matches pagedb's own create-then-`sync_dir` discipline, e.g. `bootstrap_header` at `src/pager/header.rs:58` + create-durability comment at `:147`).
- `OpfsFile`: registry token + `read_only` flag (Rust-side rejection with `PagedbError::ReadOnly`, as today at `handle.rs:82-84`); all I/O methods complete synchronously inside the async fns. `read_at` returns short reads at EOF; `read_at_vectored` zero-fills past EOF (contract at `tests/vfs_memory.rs:53-64`); `sync()` → `flush()`.
- **Vectored-write atomicity — PRD option (b) chosen (explicit carveout, not staging-and-swap):** `write_at_vectored` pre-extends to the max end-offset + performs sequential sync writes (advisory mitigations). The carveout: on error return or crash mid-sequence, earlier writes in the batch may be durable — the same physical semantics native `TokioVfs` sequential writes have; pagedb's crash atomicity comes from the CoW header flip (partial pre-header page writes land in unreachable slots and are never served; AEAD rejects torn pages), NOT from VFS write atomicity. This carveout text goes verbatim into the trait-doc clarification + Task 9 PR description, and Task 7 proves the accepted residual with a mid-vectored-write kill case. Staging-and-swap was rejected: it would copy whole page batches through scratch files on every commit for a guarantee the engine never relies on.
- `rename` = manifest entry update (Task 3) — open handles keep their registry token untouched; `remove` = manifest remove + orphan the phys ID (physical delete deferred to GC or immediate if refcount 0); `list_dir`/`mkdir_all` = manifest ops; `sync_dir` = manifest commit (NO LONGER a no-op — this is the durability point for renames/creates/removes, per `traits.rs:42-45`).
- Locks: carry over the existing `LockMap`/`OpfsLockHandle` verbatim (in-process; Web Locks are M2).
- Keep `Db<V: Vfs + Clone>` working: `OpfsVfs` stays a cheap-clone `Arc` newtype with the documented `unsafe Send/Sync + SendWrapper` justification — copy the existing safety comment forward.

**Definition of Done:**

- [ ] `harness/tests/vfs_basic.rs`: open→write→sync→reopen→read round-trip through the `Vfs` trait; `Db::open_internal(OpfsVfs, ...)` + one KV commit + drop + reopen sees the committed value — in both browsers
- [ ] `protocol.rs` and `opfs_worker.js` no longer exist; `grep -rn "OPFS_WORKER_JS\|openPersistent" ~/code/pres/vendor/pagedb/src/` returns nothing
- [ ] Native suite unaffected: full `cargo test` green in the pagedb checkout; `cargo check --target wasm32-unknown-unknown --lib --features opfs` green
- [ ] Verify: `cd ~/code/pres/vendor/pagedb && cargo test && cargo check --target wasm32-unknown-unknown --lib --features opfs` && `cd /Users/andrew/code/pres/brow/pagedb-opfs && just test-chrome && just test-firefox`

### Task 5: Port VFS conformance suite to the browser harness

**Objective:** Port every case from `tests/vfs_memory.rs` (15 tests) plus `tests/vfs_tokio.rs`'s additions (sync_dir after mutations, idempotent remove) to run against `OpfsVfs` on real OPFS in both browsers, with per-test namespace isolation. This is the "full contract" gate from the PRD — including the two cases OPFS historically couldn't do: `rename_while_open_keeps_handle_alive` and `vectored_read_zero_fills_past_eof`.

**Files:**

- Create: `harness/tests/conformance.rs`
- Test: `harness/tests/conformance.rs`

**Key Decisions / Notes:**

- Mirror test names 1:1 with `tests/vfs_memory.rs:6-185` so the mapping is auditable (`round_trip_read_write`, `vectored_read_write_round_trip`, `vectored_read_zero_fills_past_eof`, `rename_while_open_keeps_handle_alive`, `exclusive_lock_blocks_second_exclusive_same_path`, `shared_lock_coexists_then_blocks_exclusive`, `exclusive_blocks_shared_same_path`, `different_paths_are_independent_lock_domains`, `lock_releases_on_drop`, `sync_dir` behavior, `mkdir_all_is_idempotent`, `list_dir_returns_direct_children`, `truncate_shrinks_and_zero_extends`, `create_new_fails_if_exists`, `read_mode_handle_cannot_write`).
- `sync_dir_is_no_op` (memory) becomes `sync_dir_commits_namespace` here — assert semantics (post-`sync_dir` reload sees mutations), not no-op-ness.
- Each test constructs `OpfsVfs` scoped to a unique subnamespace (path prefix or fresh manifest root per test) and tears it down — OPFS state persists across tests in one browser session.
- `list_dir` order is unspecified by the trait (`traits.rs:36-37`) — sort before asserting (the memory test relies on BTreeMap order; don't import that assumption).

**Definition of Done:**

- [ ] All ported conformance cases green in headless Chromium AND Firefox, zero skips
- [ ] `rename_while_open_keeps_handle_alive` passes with writes through the pre-rename handle landing in the post-rename file
- [ ] Verify: `just test-chrome && just test-firefox`

### Task 6: Engine suites (smoke/txn/btree/segment/crash/recovery/page-sizes) in browser

**Objective:** Prove the engine-on-adapter stack: drive `Db<OpfsVfs>` through browser ports of the named native engine suites — smoke, txn_basic, btree_basic, segment_basic (create/append/seal/link/commit), crash_basic analogs (orphaned staging swept on reopen, per `tests/crash_basic.rs:9-31`), recovery_basic, and page_size_range across all five page sizes — plus a spill-file reopen loop that stresses the registry's deterministic close.

**Files:**

- Create: `harness/tests/engine.rs`
- Test: `harness/tests/engine.rs`

**Key Decisions / Notes:**

- These are ports, not reuses: native tests hardwire `MemVfs` + `#[tokio::test]`; the harness re-expresses the same flows as `#[wasm_bindgen_test]` async fns over `OpfsVfs`, keeping the native test names in comments for auditability.
- Curate to the flows that exercise the VFS contract: full-page commit cycles, segment staging→live promote (rename path!), tombstone, reopen-after-drop recovery (reconcile promotes staged segments whose live copy is missing — `src/recovery/reconcile.rs`), all five page sizes (`{4096,8192,16384,32768,65536}` per `src/txn/db/util.rs`).
- Spill stress: repeated write-txn batches that force `tmp/scratch-N` create→drop→recreate cycles (`src/txn/write.rs:136` re-opens per append) — regression net for the old fire-and-forget close race.
- Do NOT open Observer/ReadOnly modes across contexts (tokio-time hazard + M2 scope; see Context).

**Definition of Done:**

- [ ] Engine port suite green in both browsers, covering: KV commit/reopen, B+tree insert/scan, segment create→seal→promote→read-back, orphaned-staging sweep on reopen, and all five page sizes
- [ ] Spill stress case (≥20 create/drop/recreate cycles) passes without a single open failure
- [ ] Verify: `just test-chrome && just test-firefox`

### Task 7: Crash & durability oracle + native↔browser receipt parity

**Objective:** Prove crash consistency the PRD gates on: terminate the DB worker at every instrumented commit phase and inject manifest faults, asserting reopen always yields exactly the last committed state; and prove format/behavior parity by comparing a BLAKE3 receipt of committed state between native and browser runs of one shared deterministic op-script.

**Files:**

- Create: `harness/src/fault.rs` (FaultVfs wrapper + op-count cut points)
- Create: `harness/src/driver.rs` (commit-driver entry for the sacrificial worker) and `harness/js/oracle-worker.js` (thin JS glue that loads the wasm driver in a second dedicated worker)
- Create: `harness/tests/oracle.rs`
- Create: `harness/src/receipt.rs` (shared op-script + BLAKE3 receipt) and a native receipt test in the pagedb-opfs workspace (`harness/tests/receipt_native.rs`, `cfg(not(target_arch = "wasm32"))`, over `MemVfs`)
- Test: `harness/tests/oracle.rs`

**Key Decisions / Notes:**

- **Worker termination is the REQUIRED oracle mechanism for every named phase cut** (Codex spec-review finding — FaultVfs halts run destructors and error paths; real termination abandons sync handles and in-memory manifest state mid-flight, which is the semantics the goal claims). Determinism without timing games: the driver **pauses at each instrumented beacon** (posts `phase-K reached`, then awaits an ack that never comes), the test calls `Worker.terminate()` at exactly that beacon, then reopens in the test worker with a bounded retry loop (sync-handle locks release on worker death, possibly async). **FaultVfs** (wraps `OpfsVfs`, counts ops, injects typed errors at the Nth op) is supplemental: it covers the ERROR-RETURN paths (quota/I-O failure mid-batch → typed error, engine aborts cleanly, reopen serves last committed state — the carveout's error-side proof), mirroring the native pattern (`tests/crash_basic.rs`).
- Phase cut points (from `src/txn/write.rs` commit ordering), **grouped by the correct A/B-publication expectation** (Codex pass-2 finding — a kill after the higher-seq slot is written but before flush may legitimately recover the NEW state, since recovery selects the highest valid seq and the commit never returned; demanding "old state exactly" there is impossible without a durable flushed-marker):
  - **Pre-publication cuts → OLD state exactly:** mid-vectored-write (after page k of n — the PRD carveout's accepted-residual proof: partial pages never visible); after pages vectored-write pre-sync; after pages sync pre-header-write.
  - **Publication-ambiguous cuts → OLD XOR NEW, atomic either way:** after header write pre-header-sync; mid-manifest-commit (inactive slot written, not flipped). Assert invariants, not identity: the winning header/manifest slot validates (MAC/checksum), the recovered state equals exactly one of {pre-commit receipt, post-commit receipt}, and no torn/mixed state is observable.
  - **Post-publication cuts → NEW state exactly:** after header sync pre-rename; after manifest commit pre-orphan-GC.
- Manifest fault cases (beyond Task 3's): manifest/data mismatch (manifest references a phys ID whose file is missing → typed corruption/NotFound error, not a trap), rename issued during reconcile (reopen with a staged segment while manifest has pending state).
- Receipt: op-script = fixed sequence of KV puts/deletes/segment appends with fixed seeds; receipt = BLAKE3 over the ordered dump of committed KV state + segment page bytes. Native test computes the receipt over `MemVfs`; browser test computes it over `OpfsVfs`; both compare against the same expected constant (update-by-failing if the script changes).
- Every oracle case asserts BOTH: no uncommitted state visible AND all committed state present (exactly-last-committed, not merely "opens without error").

**Definition of Done:**

- [ ] Worker-terminate cases at ALL seven named phase beacons (incl. mid-vectored-write) satisfy their group's expectation in both browsers: pre-publication → old state exactly; publication-ambiguous → old XOR new with validated invariants; post-publication → new state exactly
- [ ] FaultVfs error-injection cases (quota/I-O failure mid-vectored-write and mid-manifest-commit) surface typed errors, the engine aborts cleanly, and reopen serves exactly the last committed state
- [ ] Manifest fault cases (torn slot, manifest/data mismatch, crash pre-GC, rename-during-reconcile) all recover or fail typed — never trap, never serve uncommitted state
- [ ] Receipt parity: browser receipt == native receipt for the shared op-script
- [ ] Verify: `just test-chrome && just test-firefox && cargo test -p pagedb-opfs-harness --test receipt_native`

### Task 8: Post-header metadata error-swallowing audit (upstream)

**Objective:** With `rename`/`sync_dir` now carrying manifest commits, a swallowed metadata error is a swallowed durability failure. Audit and fix all post-header metadata side-effect call sites in the pagedb branch so errors are propagated or explicitly retryable-before-completion, per the PRD's named list.

**Files:**

- Modify: `src/recovery/journal.rs` (`execute_journal_actions` → returns `Result`), `src/txn/db/segment.rs` (`sync_dir` swallow at ~:272), `src/recovery/gc.rs` (swallowed `sync_dir`) — in pagedb branch
- Modify (audit; change only where a swallow is found): `src/compaction/helpers.rs`, `src/recovery/reconcile.rs`, `src/txn/db/gc.rs`, `src/txn/db/snapshot.rs`, `src/txn/write.rs` — in pagedb branch
- Test: `tests/durability/metadata_errors.rs` (native, in pagedb branch)

**Key Decisions / Notes:**

- Audit pattern: every `mkdir_all`/`rename`/`remove`/`sync_dir` call in the listed files either `?`-propagates, or keeps an explicit comment justifying why the action stays retryable before completion state is cleared (e.g., journal actions must remain idempotent — `tests/durability/apply_journal_crash.rs` already proves idempotency).
- `execute_journal_actions` returning `Result` changes its callers (`txn/db/snapshot.rs:405` region, journal replay in open flow) — callers must NOT clear `apply_journal_root_page_id` when actions failed.
- Native test: fault-injecting VFS wrapper (MemVfs-based, in-tree test helper) that fails `rename`/`sync_dir` once → assert the operation surfaces the error (or completion state is preserved for retry) and a subsequent retry succeeds.
- Lineage guard: this task changes error plumbing only — no behavioral changes to the success paths; keep the diff minimal per file.

**Definition of Done:**

- [ ] `execute_journal_actions` returns `Result` and no caller clears journal-completion state on failure
- [ ] No unjustified `let _ =`/ignored-Result on `mkdir_all|rename|remove|sync_dir` remains in the listed files (each remaining ignore carries a written retryability justification)
- [ ] New native durability test proves a failed post-header rename/sync_dir is surfaced or retryable, then succeeds on retry
- [ ] Verify: `cd ~/code/pres/vendor/pagedb && cargo test`

### Task 9: Upstream PR draft + documentation sync

**Objective:** Package the branch as a reviewable upstream contribution: fix the documentation drift the PRD names, write the CHANGELOG entry, organize the commit series, and produce a PR description documenting the architecture, breaking changes, the single vectored-write contract carveout (Task 4's option (b), with oracle evidence), and the browser-test evidence living in the pagedb-opfs harness repo. Final state pushed to the user's Gitea; NodeDB-Lab submission stays with the user.

**Files:**

- Modify: `README.md` (line ~31: replace the false "gloo-worker on WASM/OPFS" claim with the in-worker sync-backend description), `CHANGELOG.md`, `src/vfs/opfs/mod.rs` (bootstrap docs — remove phantom `openPersistent`, document the real `OpfsVfs::new()` + dedicated-worker requirement) — in pagedb branch
- Create: `docs/pr/2026-07-opfs-sync-backend.md` (PR description, in pagedb-opfs repo)
- Modify: `docs/prd/2026-07-02-pagedb-opfs-adapter.md` (mark M1 delivery status note) — in pagedb-opfs repo

**Key Decisions / Notes:**

- PR description sections: motivation (never-executed backend, five defects with file:line), architecture (in-worker sync + manifest + registry), breaking changes (`OpfsVfs::new()` signature, `OPFS_WORKER_JS` removed, `sync_dir` semantics now real), **the vectored-write carveout** (Task 4's option-(b) text + the trait-doc clarification, with the Task 7 mid-vectored-write oracle case as evidence — this is the one contract point narrowed, replacing the earlier "none expected" assumption), test evidence (harness repo link, suites + oracle summary, both browsers), M2 roadmap pointer (Web Locks, Mode 2, bootstrap, benches, WebKit).
- Commit series: logical order (registry → manifest → rewrite → audit → docs); each commit compiles and passes native tests (rebase/squash as needed — this IS an authorized git write on the feature branch).
- CI: leave the existing `cargo check` wasm job as-is upstream; PR description notes where browser CI lives and offers it as follow-up.
- Final push of the polished branch to the user's Gitea (`git push gitea feat/opfs-sync-backend --force-with-lease` after the rebase, plus `jj git push` for pagedb-opfs) — own-Gitea pushes authorized; NodeDB-Lab submission stays with the user.

**Definition of Done:**

- [ ] `grep -n "gloo-worker" ~/code/pres/vendor/pagedb/README.md` returns nothing; mod.rs docs describe the real bootstrap
- [ ] PR description document complete with breaking-changes and test-evidence sections
- [ ] Full gate re-run green: native suite, wasm check, and both browser suites
- [ ] Verify: `cd ~/code/pres/vendor/pagedb && cargo test && cargo check --target wasm32-unknown-unknown --lib --features opfs && cd /Users/andrew/code/pres/brow/pagedb-opfs && just test-chrome && just test-firefox`

## Deferred Ideas

- **`wasm64-unknown-unknown`** (user request, correct target name confirmed): needs nightly + `-Z build-std` (Tier 3), wasm-bindgen memory64 support is experimental, Safari lacks Memory64, and pagedb gates on `target_arch = "wasm32"` throughout (a cfg audit → `target_family = "wasm"` where appropriate is the first upstream step). Revisit after M2.
- **M2 plan** (next /spec): Web Locks cross-tab single-writer + ReadOnly/Observer typed errors, quota/eviction surfacing, JS bootstrap + capability probe, Mode 2 postMessage bridge, benches, WebKit runner, PR finalization.
