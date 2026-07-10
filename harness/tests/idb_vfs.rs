//! Browser workflow proof for the experimental PageDB `IdbVfs`.

#![cfg(all(target_arch = "wasm32", feature = "idb-vendor-spike"))]

use pagedb::errors::PagedbError;
use pagedb::vfs::{IdbStore, IdbVfs, OpenMode, ReadReq, Vfs, VfsFile, WriteReq};
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

#[wasm_bindgen(inline_js = r#"
export function abort_next_idb_put() {
  const original = IDBObjectStore.prototype.put;
  IDBObjectStore.prototype.put = function (...args) {
    IDBObjectStore.prototype.put = original;
    const request = original.apply(this, args);
    this.transaction.abort();
    return request;
  };
}

export function abort_next_idb_delete() {
  const original = IDBObjectStore.prototype.delete;
  IDBObjectStore.prototype.delete = function (...args) {
    IDBObjectStore.prototype.delete = original;
    const request = original.apply(this, args);
    this.transaction.abort();
    return request;
  };
}
"#)]
extern "C" {
    fn abort_next_idb_put();
    fn abort_next_idb_delete();
}

#[wasm_bindgen_test]
async fn idb_vfs_persists_rename_while_open_across_reopen() {
    let root = format!("harness-{}", js_sys::Date::now());
    let vfs = IdbVfs::with_root(&root).await.unwrap();
    vfs.mkdir_all("segments/staged").await.unwrap();

    let mut file = vfs
        .open("segments/staged/first", OpenMode::CreateNew)
        .await
        .unwrap();
    file.write_at_vectored(&[
        WriteReq {
            offset: 0,
            buf: b"abc",
        },
        WriteReq {
            offset: 5,
            buf: b"xyz",
        },
    ])
    .await
    .unwrap();
    file.sync().await.unwrap();

    vfs.rename("segments/staged/first", "segments/live")
        .await
        .unwrap();
    file.write_at(3, b"de").await.unwrap();
    file.sync().await.unwrap();
    vfs.sync_dir("segments").await.unwrap();

    let shared = vfs.lock_shared("writer").await.unwrap();
    assert!(vfs.lock_exclusive("writer").await.is_err());
    drop(shared);
    drop(file);
    drop(vfs);

    let reopened = IdbVfs::with_root(&root).await.unwrap();
    assert_eq!(reopened.list_dir("segments").await.unwrap(), vec!["live"]);
    assert!(
        reopened
            .list_dir("segments/staged")
            .await
            .unwrap()
            .is_empty()
    );
    let reader = reopened
        .open("segments/live", OpenMode::Read)
        .await
        .unwrap();
    let mut first = [0u8; 5];
    let mut second = [9u8; 3];
    reader
        .read_at_vectored(&mut [
            ReadReq {
                offset: 0,
                buf: &mut first,
            },
            ReadReq {
                offset: 5,
                buf: &mut second,
            },
        ])
        .await
        .unwrap();
    assert_eq!(&first, b"abcde");
    assert_eq!(&second, b"xyz");

    drop(reader);
    drop(reopened);
    IdbStore::delete(&format!("pagedb-idb-vfs:{root}"))
        .await
        .unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_matches_core_open_read_and_remove_semantics() {
    let root = format!("core-{}", js_sys::Date::now());
    let vfs = IdbVfs::with_root(&root).await.unwrap();
    let mut file = vfs
        .open("nested/value", OpenMode::CreateOrOpen)
        .await
        .unwrap();
    file.write_at(0, b"abc").await.unwrap();

    assert!(vfs.open("nested/value", OpenMode::CreateNew).await.is_err());
    let mut read_only = vfs.open("nested/value", OpenMode::Read).await.unwrap();
    assert!(matches!(
        read_only.write_at(0, b"no").await,
        Err(pagedb::errors::PagedbError::ReadOnly)
    ));

    let mut bytes = [0xff; 8];
    read_only
        .read_at_vectored(&mut [ReadReq {
            offset: 0,
            buf: &mut bytes,
        }])
        .await
        .unwrap();
    assert_eq!(&bytes, b"abc\0\0\0\0\0");

    file.sync().await.unwrap();
    drop(read_only);
    drop(file);
    vfs.remove("nested/value").await.unwrap();
    vfs.sync_dir("nested").await.unwrap();
    drop(vfs);

    let reopened = IdbVfs::with_root(&root).await.unwrap();
    assert!(reopened.open("nested/value", OpenMode::Read).await.is_err());
    assert!(reopened.list_dir("nested").await.unwrap().is_empty());
    drop(reopened);
    IdbStore::delete(&format!("pagedb-idb-vfs:{root}"))
        .await
        .unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_sync_dir_is_the_metadata_visibility_boundary() {
    let root = format!("sync-dir-{}", js_sys::Date::now());
    {
        let vfs = IdbVfs::with_root(&root).await.unwrap();
        let mut durable = vfs.open("durable", OpenMode::CreateNew).await.unwrap();
        durable.write_at(0, b"d").await.unwrap();
        durable.sync().await.unwrap();
        vfs.sync_dir("/").await.unwrap();

        let mut ephemeral = vfs.open("ephemeral", OpenMode::CreateNew).await.unwrap();
        ephemeral.write_at(0, b"e").await.unwrap();
        ephemeral.sync().await.unwrap();
        // No sync_dir: this image may exist in IndexedDB but remains unnamed.
    }

    let reopened = IdbVfs::with_root(&root).await.unwrap();
    assert!(reopened.open("durable", OpenMode::Read).await.is_ok());
    assert!(reopened.open("ephemeral", OpenMode::Read).await.is_err());
    drop(reopened);
    IdbStore::delete(&format!("pagedb-idb-vfs:{root}"))
        .await
        .unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_truncate_shrinks_and_zero_extends() {
    let root = format!("truncate-{}", js_sys::Date::now());
    let vfs = IdbVfs::with_root(&root).await.unwrap();
    let mut file = vfs.open("value", OpenMode::CreateNew).await.unwrap();
    file.write_at(0, &[0xab; 100]).await.unwrap();

    file.truncate(50).await.unwrap();
    assert_eq!(file.len().await.unwrap(), 50);
    file.truncate(150).await.unwrap();
    assert_eq!(file.len().await.unwrap(), 150);

    let mut extension = [0xff; 100];
    assert_eq!(file.read_at(50, &mut extension).await.unwrap(), 100);
    assert!(extension.iter().all(|byte| *byte == 0));

    drop(file);
    drop(vfs);
    IdbStore::delete(&format!("pagedb-idb-vfs:{root}"))
        .await
        .unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_lists_direct_files_and_removes_idempotently() {
    let root = format!("list-remove-{}", js_sys::Date::now());
    let vfs = IdbVfs::with_root(&root).await.unwrap();
    vfs.mkdir_all("d/sub").await.unwrap();
    vfs.mkdir_all("d/sub").await.unwrap();

    for path in ["d/a", "d/b", "d/sub/deep"] {
        vfs.open(path, OpenMode::CreateNew).await.unwrap();
    }
    let mut entries = vfs.list_dir("d").await.unwrap();
    entries.sort();
    assert_eq!(entries, vec!["a", "b"]);

    vfs.remove("d/a").await.unwrap();
    vfs.remove("/d/a").await.unwrap();
    assert!(vfs.open("d/a", OpenMode::Read).await.is_err());

    drop(vfs);
    IdbStore::delete(&format!("pagedb-idb-vfs:{root}"))
        .await
        .unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_matches_reference_lock_matrix() {
    let root = format!("lock-matrix-{}", js_sys::Date::now());
    let database = format!("pagedb-idb-vfs:{root}");
    let vfs = IdbVfs::with_root(&root).await.unwrap();

    let shared_first = vfs.lock_shared("shared").await.unwrap();
    let shared_second = vfs.lock_shared("shared").await.unwrap();
    assert!(matches!(
        vfs.lock_exclusive("shared").await,
        Err(PagedbError::AlreadyLocked)
    ));
    drop(shared_second);
    drop(shared_first);

    let exclusive = vfs.lock_exclusive("exclusive").await.unwrap();
    assert!(matches!(
        vfs.lock_shared("exclusive").await,
        Err(PagedbError::AlreadyLocked)
    ));
    let independent = vfs.lock_exclusive("independent").await.unwrap();
    drop(exclusive);
    let reacquired = vfs.lock_exclusive("exclusive").await.unwrap();

    drop(reacquired);
    drop(independent);
    drop(vfs);
    IdbStore::delete(&database).await.unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_matches_reference_create_or_open_and_absent_read_write_semantics() {
    let root = format!("open-modes-{}", js_sys::Date::now());
    let database = format!("pagedb-idb-vfs:{root}");
    let vfs = IdbVfs::with_root(&root).await.unwrap();

    let error = match vfs.open("missing", OpenMode::ReadWrite).await {
        Ok(_) => panic!("ReadWrite unexpectedly created an absent file"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        PagedbError::Io(ref io_error) if io_error.kind() == std::io::ErrorKind::NotFound
    ));

    let mut first = vfs.open("value", OpenMode::CreateOrOpen).await.unwrap();
    first.write_at(0, b"keep me").await.unwrap();
    drop(first);
    let second = vfs.open("value", OpenMode::CreateOrOpen).await.unwrap();
    assert_eq!(second.len().await.unwrap(), 7);

    drop(second);
    drop(vfs);
    IdbStore::delete(&database).await.unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_aborted_file_sync_preserves_the_last_committed_image() {
    let root = format!("abort-file-{}", js_sys::Date::now());
    {
        let vfs = IdbVfs::with_root(&root).await.unwrap();
        let mut file = vfs.open("value", OpenMode::CreateNew).await.unwrap();
        file.write_at(0, b"old").await.unwrap();
        file.sync().await.unwrap();
        vfs.sync_dir("/").await.unwrap();

        file.write_at(0, b"new").await.unwrap();
        abort_next_idb_put();
        assert!(matches!(file.sync().await, Err(PagedbError::Io(_))));
    }

    let reopened = IdbVfs::with_root(&root).await.unwrap();
    let reader = reopened.open("value", OpenMode::Read).await.unwrap();
    let mut bytes = [0; 3];
    assert_eq!(reader.read_at(0, &mut bytes).await.unwrap(), 3);
    assert_eq!(&bytes, b"old");

    drop(reader);
    drop(reopened);
    IdbStore::delete(&format!("pagedb-idb-vfs:{root}"))
        .await
        .unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_aborted_namespace_sync_keeps_new_paths_unpublished() {
    let root = format!("abort-namespace-{}", js_sys::Date::now());
    {
        let vfs = IdbVfs::with_root(&root).await.unwrap();
        let mut file = vfs.open("unpublished", OpenMode::CreateNew).await.unwrap();
        file.write_at(0, b"value").await.unwrap();
        file.sync().await.unwrap();

        abort_next_idb_put();
        assert!(matches!(vfs.sync_dir("/").await, Err(PagedbError::Io(_))));
    }

    let reopened = IdbVfs::with_root(&root).await.unwrap();
    assert!(reopened.open("unpublished", OpenMode::Read).await.is_err());

    drop(reopened);
    IdbStore::delete(&format!("pagedb-idb-vfs:{root}"))
        .await
        .unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_sync_dir_collects_unreferenced_file_records() {
    let root = format!("orphan-gc-{}", js_sys::Date::now());
    let database = format!("pagedb-idb-vfs:{root}");
    let vfs = IdbVfs::with_root(&root).await.unwrap();
    let mut live = vfs.open("live", OpenMode::CreateNew).await.unwrap();
    live.write_at(0, b"live").await.unwrap();
    live.sync().await.unwrap();
    vfs.sync_dir("/").await.unwrap();

    let store = IdbStore::open(&database).await.unwrap();
    store.store_file(999, b"orphan").await.unwrap();
    assert_eq!(
        store.load_file(999).await.unwrap(),
        Some(b"orphan".to_vec())
    );
    store.close();

    vfs.sync_dir("/").await.unwrap();

    let store = IdbStore::open(&database).await.unwrap();
    assert_eq!(store.load_file(0).await.unwrap(), Some(b"live".to_vec()));
    assert_eq!(store.load_file(999).await.unwrap(), None);
    store.close();
    drop(live);
    drop(vfs);
    IdbStore::delete(&database).await.unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_retries_orphan_cleanup_after_an_aborted_sweep() {
    let root = format!("orphan-retry-{}", js_sys::Date::now());
    let database = format!("pagedb-idb-vfs:{root}");
    let vfs = IdbVfs::with_root(&root).await.unwrap();
    let mut live = vfs.open("live", OpenMode::CreateNew).await.unwrap();
    live.write_at(0, b"live").await.unwrap();
    live.sync().await.unwrap();
    vfs.sync_dir("/").await.unwrap();

    let store = IdbStore::open(&database).await.unwrap();
    store.store_file(999, b"orphan").await.unwrap();
    store.close();

    abort_next_idb_delete();
    vfs.sync_dir("/").await.unwrap();

    let store = IdbStore::open(&database).await.unwrap();
    assert_eq!(
        store.load_file(999).await.unwrap(),
        Some(b"orphan".to_vec())
    );
    store.close();

    vfs.sync_dir("/").await.unwrap();

    let store = IdbStore::open(&database).await.unwrap();
    assert_eq!(store.load_file(0).await.unwrap(), Some(b"live".to_vec()));
    assert_eq!(store.load_file(999).await.unwrap(), None);
    store.close();
    drop(live);
    drop(vfs);
    IdbStore::delete(&database).await.unwrap();
}
