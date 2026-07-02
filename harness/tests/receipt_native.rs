//! Native side of the receipt-parity oracle: run the shared op-script on
//! MemVfs (the reference backend) and pin its BLAKE3 receipt. The browser
//! side (tests/receipt_browser.rs) must produce the identical string over
//! OpfsVfs - receipt equality means the engine observed the same committed
//! state through both backends.

#![cfg(not(target_arch = "wasm32"))]

use pagedb::vfs::memory::MemVfs;
use pagedb::{Db, RealmId};
use pagedb_opfs_harness::receipt::{self, EXPECTED_RECEIPT, KEK, PAGE};

const REALM: RealmId = receipt::REALM;

#[tokio::test(flavor = "current_thread")]
async fn native_receipt_matches_pinned_constant() {
    let db = Db::open_internal(MemVfs::new(), KEK, PAGE, REALM)
        .await
        .unwrap();
    receipt::run_script(&db).await;
    let got = receipt::compute_receipt(&db).await;
    assert_eq!(
        got, EXPECTED_RECEIPT,
        "native receipt drifted - if the op-script changed intentionally, \
         re-pin EXPECTED_RECEIPT in harness/src/receipt.rs"
    );
}
