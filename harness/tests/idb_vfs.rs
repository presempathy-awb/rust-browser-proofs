//! Browser workflow proof for the experimental PageDB `IdbVfs`.

#![cfg(all(target_arch = "wasm32", feature = "idb-vendor-spike"))]

use pagedb::vfs::{IdbStore, IdbVfs, OpenMode, ReadReq, Vfs, VfsFile, WriteReq};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

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
