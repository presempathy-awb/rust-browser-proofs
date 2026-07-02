//! Browser side of the receipt-parity oracle: the shared op-script over
//! OpfsVfs on real OPFS must produce the receipt pinned by the native
//! MemVfs reference run - byte-identical committed state through both
//! backends, including across a full reopen.

#![cfg(target_arch = "wasm32")]

mod support;

use pagedb::vfs::opfs::OpfsVfs;
use pagedb::{Db, RealmId};
use pagedb_opfs_harness::receipt::{self, EXPECTED_RECEIPT, KEK, PAGE};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const REALM: RealmId = receipt::REALM;

#[wasm_bindgen_test]
async fn browser_receipt_matches_native_reference() {
    support::cleanup_dir("receipt-parity").await;
    {
        let vfs = OpfsVfs::with_root("receipt-parity").await.unwrap();
        let db = Db::open_internal(vfs, KEK, PAGE, REALM).await.unwrap();
        receipt::run_script(&db).await;
        let live = receipt::compute_receipt(&db).await;
        assert_eq!(live, EXPECTED_RECEIPT, "live browser receipt diverged");
    }
    // The receipt must also survive a full reopen (bytes from OPFS alone).
    let vfs = OpfsVfs::with_root("receipt-parity").await.unwrap();
    let db = Db::open_existing(vfs, KEK, PAGE, REALM).await.unwrap();
    let reopened = receipt::compute_receipt(&db).await;
    assert_eq!(
        reopened, EXPECTED_RECEIPT,
        "reopened browser receipt diverged"
    );
}
