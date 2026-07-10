//! pagedb-opfs harness library.
//!
//! Test-support code shared by the browser suites: the fault-injection VFS
//! wrapper, the beacon VFS + sacrificial-worker crash-oracle driver, and the
//! deterministic receipt op-script. The suites live under `tests/` and run
//! via `wasm-pack test --headless` inside a dedicated Web Worker (OPFS sync
//! access handles are unavailable elsewhere).

pub mod receipt;

#[cfg(target_arch = "wasm32")]
pub mod fault;

#[cfg(target_arch = "wasm32")]
pub mod driver;

#[cfg(all(target_arch = "wasm32", feature = "idb-crash-driver"))]
pub mod idb_driver;
