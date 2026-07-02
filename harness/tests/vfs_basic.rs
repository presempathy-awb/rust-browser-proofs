//! Task 4 RED->GREEN: the rewritten in-worker OpfsVfs.
//!
//! Observable contract:
//! - `OpfsVfs::with_root(root).await` constructs WITHOUT any worker URL or
//!   postMessage machinery, inside the dedicated worker itself.
//! - The `Vfs`/`VfsFile` trait surface round-trips durably (write -> sync ->
//!   sync_dir -> drop everything -> fresh VFS on the same root -> read).
//! - `Db<OpfsVfs>` opens, commits, and reopens a real database in the
//!   browser - including the engine's double-open of `/main.db` that the
//!   registry exists to make legal.

#![cfg(target_arch = "wasm32")]

mod support;

use pagedb::vfs::opfs::OpfsVfs;
use pagedb::vfs::{OpenMode, Vfs, VfsFile};
use pagedb::{Db, RealmId};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const PAGE: usize = 4096;

#[wasm_bindgen_test]
async fn trait_round_trip_survives_vfs_reopen() {
    support::cleanup_dir("vfs-basic-trait").await;

    {
        let vfs = OpfsVfs::with_root("vfs-basic-trait").await.unwrap();
        let mut f = vfs.open("/x.bin", OpenMode::CreateNew).await.unwrap();
        assert_eq!(f.write_at(0, b"hello opfs").await.unwrap(), 10);
        f.sync().await.unwrap();
        vfs.sync_dir("/").await.unwrap(); // durability point for the create
    }

    // Everything dropped: registry handles closed, manifest slots closed.
    let vfs = OpfsVfs::with_root("vfs-basic-trait").await.unwrap();
    let f = vfs.open("/x.bin", OpenMode::Read).await.unwrap();
    assert_eq!(f.len().await.unwrap(), 10);
    let mut buf = [0u8; 10];
    let n = f.read_at(0, &mut buf).await.unwrap();
    assert_eq!(n, 10);
    assert_eq!(&buf, b"hello opfs");

    // Read-mode handles reject writes.
    let mut ro = vfs.open("/x.bin", OpenMode::Read).await.unwrap();
    assert!(matches!(
        ro.write_at(0, b"nope").await,
        Err(pagedb::errors::PagedbError::ReadOnly)
    ));
}

#[wasm_bindgen_test]
async fn db_open_commit_reopen_sees_committed_value() {
    support::cleanup_dir("vfs-basic-db").await;

    {
        let vfs = OpfsVfs::with_root("vfs-basic-db").await.unwrap();
        let db = Db::open_internal(vfs, [9u8; 32], PAGE, RealmId::new([1; 16]))
            .await
            .unwrap();
        let mut w = db.begin_write().await.unwrap();
        w.put(b"k", b"v-browser").await.unwrap();
        w.commit().await.unwrap();
    }

    let vfs = OpfsVfs::with_root("vfs-basic-db").await.unwrap();
    let db = Db::open_existing(vfs, [9u8; 32], PAGE, RealmId::new([1; 16]))
        .await
        .unwrap();
    let r = db.begin_read().await.unwrap();
    assert_eq!(
        r.get(b"k").await.unwrap().as_deref(),
        Some(b"v-browser".as_ref())
    );
}
