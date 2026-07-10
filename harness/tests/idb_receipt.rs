//! Receipt parity for the experimental, local-only PageDB `IdbVfs`.

#![cfg(all(target_arch = "wasm32", feature = "idb-vendor-spike"))]

use pagedb::vfs::{IdbStore, IdbVfs};
use pagedb::{Db, RealmId};
use pagedb_opfs_harness::receipt::{self, EXPECTED_RECEIPT, KEK, PAGE};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const REALM: RealmId = receipt::REALM;

#[wasm_bindgen_test]
async fn idb_receipt_matches_native_reference_across_reopen() {
    let root = format!("receipt-{}", js_sys::Date::now());
    {
        let vfs = IdbVfs::with_root(&root).await.unwrap();
        let db = Db::open_internal(vfs, KEK, PAGE, REALM).await.unwrap();
        receipt::run_script(&db).await;
        assert_eq!(
            receipt::compute_receipt(&db).await,
            EXPECTED_RECEIPT,
            "live IDB receipt diverged"
        );
    }

    {
        let vfs = IdbVfs::with_root(&root).await.unwrap();
        let db = Db::open_existing(vfs, KEK, PAGE, REALM).await.unwrap();
        assert_eq!(
            receipt::compute_receipt(&db).await,
            EXPECTED_RECEIPT,
            "reopened IDB receipt diverged"
        );
    }

    IdbStore::delete(&format!("pagedb-idb-vfs:{root}"))
        .await
        .unwrap();
}
