//! Browser integration proof for PageDB's local-only IDB persistence spike.

#![cfg(all(target_arch = "wasm32", feature = "idb-vendor-spike"))]

use pagedb::vfs::idb::IdbStore;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

#[wasm_bindgen_test]
async fn idb_store_commits_file_and_namespace_in_one_transaction() {
    let name = format!("pagedb-idb-store-{}", js_sys::Date::now());
    let store = IdbStore::open(&name).await.unwrap();

    store
        .commit_file_and_namespace(42, &[4, 2, 4, 2], b"/root/live=42")
        .await
        .unwrap();

    assert_eq!(store.load_file(42).await.unwrap(), Some(vec![4, 2, 4, 2]));
    assert_eq!(
        store.load_namespace().await.unwrap(),
        Some(b"/root/live=42".to_vec())
    );
    assert_eq!(store.load_file(43).await.unwrap(), None);

    store.close();
    IdbStore::delete(&name).await.unwrap();
}
