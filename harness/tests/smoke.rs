//! PageDB consumes the generic OPFS round-trip battery before any backend test.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

#[wasm_bindgen_test]
async fn sync_access_handle_round_trip_in_dedicated_worker() {
    rust_browser_proofs::opfs::assert_sync_access_handle_round_trip()
        .await
        .expect("generic OPFS sync-access-handle round trip failed");
}
