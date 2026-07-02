//! pagedb-opfs harness library.
//!
//! Test-support code shared by the browser suites lives here (fault-injection
//! VFS wrapper, crash-oracle driver, receipt op-script). The suites themselves
//! live under `tests/` and run via `wasm-pack test --headless` inside a
//! dedicated Web Worker (OPFS sync access handles are unavailable elsewhere).
