//! Dev-only browser proof batteries for Rust/Wasm projects.
//!
//! This crate is intentionally a test dependency. It never selects a storage
//! backend or becomes part of a consumer's runtime dependency graph.

#[cfg(target_arch = "wasm32")]
pub mod opfs;

/// Emit the generic dedicated-worker OPFS battery in the calling test crate.
///
/// Invoke this macro once in a wasm-only integration test file. The generated
/// tests prove raw sync-handle persistence and a small write+flush/read
/// baseline. They do not make any backend-specific durability claim.
#[macro_export]
macro_rules! opfs_worker_battery {
    () => {
        wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn rust_browser_proofs_sync_access_handle_round_trip() {
            $crate::opfs::assert_sync_access_handle_round_trip()
                .await
                .expect("generic OPFS sync-access-handle round trip failed");
        }

        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn rust_browser_proofs_raw_sync_baseline() {
            $crate::opfs::assert_raw_sync_baseline()
                .await
                .expect("generic OPFS raw sync baseline failed");
        }
    };
}
