//! PageDB consumes the generic raw OPFS sync-handle baseline.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

#[wasm_bindgen_test]
async fn raw_sync_benchmark_reports_completed_read_and_write_work() {
    rust_browser_proofs::opfs::assert_raw_sync_baseline()
        .await
        .expect("generic OPFS raw sync baseline failed");
}

#[wasm_bindgen_test]
async fn raw_sync_benchmark_rejects_invalid_workload_dimensions() {
    assert!(
        rust_browser_proofs::opfs::benchmark_raw_sync_access_handle(0, 1)
            .await
            .is_err()
    );
}
