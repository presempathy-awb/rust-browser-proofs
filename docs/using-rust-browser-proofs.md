# Using Rust Browser Proofs From Another Repository

This note is a copy-ready local-development handoff for sibling repositories.

## Current Local Locations

- Canonical source checkout: `/Users/andrew/code/pres/brow/rust-browser-proofs`
- Active PageDB integration checkout: `/Users/andrew/Documents/silopal-pagedb-opfs/browser-proofs`
- Parent integration repository: `/Users/andrew/Documents/silopal-pagedb-opfs`

The source checkout is the place to develop shared runner and documentation
changes. The active integration checkout is the submodule currently executed by
the PageDB parent repository.

## Use The Test Crate

For a sibling Rust/Wasm repository that needs the generic browser battery today:

```toml
[target.'cfg(target_arch = "wasm32")'.dev-dependencies]
rust-browser-proofs = { path = "/Users/andrew/code/pres/brow/rust-browser-proofs/crates/rust-browser-proofs" }
wasm-bindgen-test = "0.3"
```

Then add one wasm-only integration test file:

```rust
#![cfg(target_arch = "wasm32")]

rust_browser_proofs::opfs_worker_battery!();
```

The macro emits the test functions in your crate. Cargo does not automatically
run integration tests shipped by a dependency, so the one-line entrypoint is
the intentional handoff boundary. `wasm-bindgen-test` stays a direct test
dependency because it generates the browser-test glue in that crate.

For a hosted sibling repository, replace the absolute local path with a pinned
Git dependency. Keep this crate under `dev-dependencies`:

```toml
[target.'cfg(target_arch = "wasm32")'.dev-dependencies]
rust-browser-proofs = { git = "<your hosted rust-browser-proofs URL>", rev = "<validated revision>" }
wasm-bindgen-test = "0.3"
```

Do not publish the absolute `/Users/andrew/...` path. It exists only for this
machine's local-development handoff.

## Invoke It From Your Repository

Run the generated tests with your own browser recipe:

```just
test-opfs-battery:
    rust-browser-proofs -- wasm-pack test --headless --chrome
```

`rust-browser-proofs -- <command>` forwards a normal test command with
Rustup's selected `rustc` and `cargo` first in the child environment. It does
not inspect, rewrite, or choose browser flags. The proof repository's
`justfile` remains available for its own Chrome, Firefox, Safari, Android
Chrome, and iPhone Safari runners. A new consumer should own named wrapper
recipes for its own browser matrix rather than inherit PageDB's database-
specific selection policy.

The command preserves the current directory. Run `wasm-pack` from the consumer
package directory; if a consumer repository has a virtual workspace root,
change into the member containing `[package]` before invoking the recipe.

To capture a Markdown environment snapshot without running a test, request a
report:

```sh
rust-browser-proofs --report
```

Place `--report` before `-- <command>` to record one command's exit result as
well. The report distinguishes local prerequisites from test evidence:
only an explicit successful `wasm-pack` browser flag marks that desktop browser
passed, and all other browser/device targets remain unexecuted.

Default reports are timestamped files in
`$RUST_BROWSER_PROOFS_REPORT_DIR` when set, otherwise
`$XDG_CACHE_HOME/rust-browser-proofs/browser-tests` or
`$HOME/cache/rust-browser-proofs/browser-tests`. Pass `--report <path>` when a
CI job or handoff needs a specific artifact location.

The report directory also contains `report-cache.sqlite3`. The native runner
stores the exact Markdown in one transactionally upserted `report_cache` row per
report path, with its write time. Run `rust-browser-proofs --mirror-report
<path>` when an existing Markdown artifact must be added to that local cache.

In the canonical checkout, run `mise trust .mise.toml` first; its checked-in
`bin/` entrypoint makes the command available without a global installation.
For a sibling repository, either add that same `bin/` directory to its
project-local PATH or use the explicit `cargo run --manifest-path` command in
the checklist. A global `cargo install --path crates/rust-browser-proofs --features runner` is optional and must be rerun
after local runner changes.

## What Is Reusable Today

- The `rust-browser-proofs` dev-only test crate and `opfs_worker_battery!()`.
- Dedicated-worker browser execution conventions for Rust/Wasm.
- Desktop, Safari, Android, and iPhone runner recipes.
- OPFS capability and raw-sync benchmark patterns.
- Source-owned durability acceptance criteria and pinned revisions.

## What Is PageDB-Specific

- The `pagedb-opfs-harness` Cargo package.
- OPFS/IDB storage semantics, crash-oracle tests, and receipt assertions.
- The PageDB private Git dependency and local `.cargo/config.toml` patch.

Do **not** add `pagedb-opfs-harness` as any dependency. A non-PageDB consumer
uses `rust-browser-proofs` only in its test target, then owns its backend
adapter, browser namespace cleanup, and acceptance matrix.

## Verify Your Integration

```sh
cargo check --target wasm32-unknown-unknown --tests
wasm-pack test --headless --chrome
```

See [`docs/browser-environment-checklist.md`](browser-environment-checklist.md)
for the current evidence checklist and incomplete browser/device gates.

For PageDB development, the module's ignored local Cargo patch resolves the
vendor checkout at `/Users/andrew/code/pres/vendor/pagedb`. Other consumers
should keep any equivalent local patch ignored and must not commit a
machine-specific path.
