//! Browser side of the receipt-parity oracle: the shared op-script over
//! OpfsVfs on real OPFS must produce the receipt pinned by the native
//! MemVfs reference run - byte-identical committed state through both
//! backends, including across a full reopen.

#![cfg(target_arch = "wasm32")]

mod support;

use pagedb::vfs::opfs::OpfsVfs;
use pagedb::{Db, RealmId};
use pagedb_opfs_harness::receipt::{self, KEK, RECEIPT_MATRIX};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const REALM: RealmId = receipt::REALM;

#[wasm_bindgen_test]
async fn browser_receipt_matches_native_reference_for_all_legal_page_sizes() {
    for (page, expected) in RECEIPT_MATRIX {
        let root = format!("receipt-parity-{page}");
        support::cleanup_dir(&root).await;
        {
            let vfs = OpfsVfs::with_root(&root).await.unwrap();
            let db = Db::open_internal(vfs, KEK, page, REALM).await.unwrap();
            receipt::run_script(&db).await;
            let live = receipt::compute_receipt(&db).await;
            assert_eq!(
                live, expected,
                "live browser receipt diverged for page size {page}"
            );
        }
        // The receipt must also survive a full reopen (bytes from OPFS alone).
        let vfs = OpfsVfs::with_root(&root).await.unwrap();
        let db = Db::open_existing(vfs, KEK, page, REALM).await.unwrap();
        let reopened = receipt::compute_receipt(&db).await;
        assert_eq!(
            reopened, expected,
            "reopened browser receipt diverged for page size {page}"
        );
    }
}
