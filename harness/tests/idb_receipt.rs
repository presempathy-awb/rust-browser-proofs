//! Receipt parity for the experimental, local-only PageDB `IdbVfs`.

#![cfg(all(target_arch = "wasm32", feature = "idb-vendor-spike"))]

use pagedb::vfs::{IdbStore, IdbVfs};
use pagedb::{Db, RealmId};
use pagedb_opfs_harness::receipt::{self, KEK, RECEIPT_MATRIX};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const REALM: RealmId = receipt::REALM;

#[wasm_bindgen_test]
async fn idb_receipt_matches_native_reference_across_reopen_for_all_legal_page_sizes() {
    for (page, expected) in RECEIPT_MATRIX {
        let root = format!("receipt-{page}-{}", js_sys::Date::now());
        {
            let vfs = IdbVfs::with_root(&root).await.unwrap();
            let db = Db::open_internal(vfs, KEK, page, REALM).await.unwrap();
            receipt::run_script(&db).await;
            assert_eq!(
                receipt::compute_receipt(&db).await,
                expected,
                "live IDB receipt diverged for page size {page}"
            );
        }

        {
            let vfs = IdbVfs::with_root(&root).await.unwrap();
            let db = Db::open_existing(vfs, KEK, page, REALM).await.unwrap();
            assert_eq!(
                receipt::compute_receipt(&db).await,
                expected,
                "reopened IDB receipt diverged for page size {page}"
            );
        }

        IdbStore::delete(&format!("pagedb-idb-vfs:{root}"))
            .await
            .unwrap();
    }
}
