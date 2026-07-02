//! Task 5: the VFS conformance suite, ported 1:1 from pagedb's
//! `tests/vfs_memory.rs` (MemVfs is the documented reference semantic) plus
//! the `tests/vfs_tokio.rs` extras, run against `OpfsVfs` on real OPFS.
//!
//! Test names mirror the originals so the mapping is auditable. Each test
//! gets its own OPFS root (`conf-<name>`); state persists within one
//! browser session, so isolation comes from distinct roots + pre-clean.
//!
//! One semantic replacement: MemVfs's `sync_dir_is_no_op` becomes
//! `sync_dir_commits_namespace` - on this backend `sync_dir` IS the
//! metadata durability point, so the port asserts the semantics, not the
//! no-op-ness.

#![cfg(target_arch = "wasm32")]

mod support;

use pagedb::errors::PagedbError;
use pagedb::vfs::opfs::OpfsVfs;
use pagedb::vfs::{OpenMode, ReadReq, Vfs, VfsFile, WriteReq};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

async fn fresh_vfs(root: &str) -> OpfsVfs {
    support::cleanup_dir(root).await;
    OpfsVfs::with_root(root).await.unwrap()
}

// ── tests/vfs_memory.rs ports ─────────────────────────────────────────────────

#[wasm_bindgen_test]
async fn round_trip_read_write() {
    let vfs = fresh_vfs("conf-roundtrip").await;
    let mut f = vfs.open("/x", OpenMode::CreateNew).await.unwrap();
    f.write_at(0, b"hello").await.unwrap();
    f.write_at(5, b" world").await.unwrap();

    let g = vfs.open("/x", OpenMode::Read).await.unwrap();
    let mut buf = vec![0u8; 11];
    let n = g.read_at(0, &mut buf).await.unwrap();
    assert_eq!(n, 11);
    assert_eq!(&buf, b"hello world");
}

#[wasm_bindgen_test]
async fn vectored_read_write_round_trip() {
    let vfs = fresh_vfs("conf-vectored").await;
    let mut f = vfs.open("/x", OpenMode::CreateNew).await.unwrap();
    let w = vec![
        WriteReq {
            offset: 0,
            buf: b"AAAA",
        },
        WriteReq {
            offset: 100,
            buf: b"BBBB",
        },
    ];
    f.write_at_vectored(&w).await.unwrap();

    let mut a = [0u8; 4];
    let mut b = [0u8; 4];
    let mut reqs = [
        ReadReq {
            offset: 0,
            buf: &mut a,
        },
        ReadReq {
            offset: 100,
            buf: &mut b,
        },
    ];
    f.read_at_vectored(&mut reqs).await.unwrap();
    assert_eq!(&a, b"AAAA");
    assert_eq!(&b, b"BBBB");
}

#[wasm_bindgen_test]
async fn vectored_read_zero_fills_past_eof() {
    let vfs = fresh_vfs("conf-zerofill").await;
    let mut f = vfs.open("/x", OpenMode::CreateNew).await.unwrap();
    f.write_at(0, b"abc").await.unwrap();
    let mut buf = [0xffu8; 8];
    let mut reqs = [ReadReq {
        offset: 0,
        buf: &mut buf,
    }];
    f.read_at_vectored(&mut reqs).await.unwrap();
    assert_eq!(&buf, b"abc\0\0\0\0\0");
}

#[wasm_bindgen_test]
async fn rename_while_open_keeps_handle_alive() {
    let vfs = fresh_vfs("conf-rename-open").await;
    let mut f = vfs.open("/from", OpenMode::CreateNew).await.unwrap();
    f.write_at(0, b"first").await.unwrap();
    vfs.rename("/from", "/to").await.unwrap();
    // Open handle still works.
    f.write_at(5, b" second").await.unwrap();

    // The renamed path has both writes.
    let g = vfs.open("/to", OpenMode::Read).await.unwrap();
    let mut buf = vec![0u8; 12];
    let n = g.read_at(0, &mut buf).await.unwrap();
    assert_eq!(n, 12);
    assert_eq!(&buf, b"first second");

    // /from is gone.
    let err = vfs
        .open("/from", OpenMode::Read)
        .await
        .err()
        .expect("opened gone path");
    assert!(matches!(err, PagedbError::Io(_)));
}

#[wasm_bindgen_test]
async fn exclusive_lock_blocks_second_exclusive_same_path() {
    let vfs = fresh_vfs("conf-lock-xx").await;
    let _a = vfs.lock_exclusive("/p").await.unwrap();
    let err = vfs.lock_exclusive("/p").await.err().unwrap();
    assert!(matches!(err, PagedbError::AlreadyLocked));
}

#[wasm_bindgen_test]
async fn shared_lock_coexists_then_blocks_exclusive() {
    let vfs = fresh_vfs("conf-lock-ssx").await;
    let _a = vfs.lock_shared("/p").await.unwrap();
    let _b = vfs.lock_shared("/p").await.unwrap();
    let err = vfs.lock_exclusive("/p").await.err().unwrap();
    assert!(matches!(err, PagedbError::AlreadyLocked));
}

#[wasm_bindgen_test]
async fn exclusive_blocks_shared_same_path() {
    let vfs = fresh_vfs("conf-lock-xs").await;
    let _a = vfs.lock_exclusive("/p").await.unwrap();
    let err = vfs.lock_shared("/p").await.err().unwrap();
    assert!(matches!(err, PagedbError::AlreadyLocked));
}

#[wasm_bindgen_test]
async fn different_paths_are_independent_lock_domains() {
    let vfs = fresh_vfs("conf-lock-indep").await;
    let _a = vfs.lock_exclusive("/p1").await.unwrap();
    let _b = vfs.lock_exclusive("/p2").await.unwrap();
}

#[wasm_bindgen_test]
async fn lock_releases_on_drop() {
    let vfs = fresh_vfs("conf-lock-drop").await;
    {
        let _a = vfs.lock_exclusive("/p").await.unwrap();
    }
    let _b = vfs.lock_exclusive("/p").await.unwrap();
}

/// Port of `sync_dir_is_no_op`, upgraded to this backend's real semantics:
/// metadata (creates/renames/removes) becomes durable at `sync_dir`.
#[wasm_bindgen_test]
async fn sync_dir_commits_namespace() {
    support::cleanup_dir("conf-syncdir").await;
    {
        let vfs = OpfsVfs::with_root("conf-syncdir").await.unwrap();
        let mut f = vfs.open("/durable", OpenMode::CreateNew).await.unwrap();
        f.write_at(0, b"d").await.unwrap();
        f.sync().await.unwrap();
        vfs.sync_dir("/").await.unwrap();

        let mut g = vfs.open("/ephemeral", OpenMode::CreateNew).await.unwrap();
        g.write_at(0, b"e").await.unwrap();
        g.sync().await.unwrap();
        // No sync_dir for /ephemeral - its manifest entry is uncommitted.
    }
    let vfs = OpfsVfs::with_root("conf-syncdir").await.unwrap();
    assert!(vfs.open("/durable", OpenMode::Read).await.is_ok());
    assert!(vfs.open("/ephemeral", OpenMode::Read).await.is_err());
}

#[wasm_bindgen_test]
async fn mkdir_all_is_idempotent() {
    let vfs = fresh_vfs("conf-mkdir").await;
    vfs.mkdir_all("/a/b/c").await.unwrap();
    vfs.mkdir_all("/a/b/c").await.unwrap();
}

#[wasm_bindgen_test]
async fn list_dir_returns_direct_children() {
    let vfs = fresh_vfs("conf-listdir").await;
    vfs.open("/d/a", OpenMode::CreateNew).await.unwrap();
    vfs.open("/d/b", OpenMode::CreateNew).await.unwrap();
    vfs.open("/d/sub/deep", OpenMode::CreateNew).await.unwrap();
    let mut entries = vfs.list_dir("/d").await.unwrap();
    entries.sort(); // trait: order unspecified
    // 1:1 with vfs_memory.rs: direct FILE children only - the implied
    // "sub" directory is not listed by the reference semantic.
    assert_eq!(entries, vec!["a".to_string(), "b".to_string()]);
}

#[wasm_bindgen_test]
async fn truncate_shrinks_and_zero_extends() {
    let vfs = fresh_vfs("conf-truncate").await;
    let mut f = vfs.open("/x", OpenMode::CreateNew).await.unwrap();
    f.write_at(0, &[0xab; 100]).await.unwrap();
    f.truncate(50).await.unwrap();
    assert_eq!(f.len().await.unwrap(), 50);
    f.truncate(150).await.unwrap();
    assert_eq!(f.len().await.unwrap(), 150);
    let mut buf = vec![0xff; 100];
    let n = f.read_at(50, &mut buf).await.unwrap();
    assert_eq!(n, 100);
    assert!(buf.iter().all(|b| *b == 0));
}

#[wasm_bindgen_test]
async fn create_new_fails_if_exists() {
    let vfs = fresh_vfs("conf-createnew").await;
    vfs.open("/x", OpenMode::CreateNew).await.unwrap();
    let err = vfs.open("/x", OpenMode::CreateNew).await.err().unwrap();
    assert!(matches!(err, PagedbError::Io(_)));
}

#[wasm_bindgen_test]
async fn read_mode_handle_cannot_write() {
    let vfs = fresh_vfs("conf-readonly").await;
    vfs.open("/x", OpenMode::CreateNew).await.unwrap();
    let mut g = vfs.open("/x", OpenMode::Read).await.unwrap();
    let err = g.write_at(0, b"nope").await.err().unwrap();
    assert!(matches!(err, PagedbError::ReadOnly));
}

// ── tests/vfs_tokio.rs extras ─────────────────────────────────────────────────

#[wasm_bindgen_test]
async fn remove_is_idempotent() {
    let vfs = fresh_vfs("conf-remove").await;
    vfs.open("/x", OpenMode::CreateNew).await.unwrap();
    vfs.remove("/x").await.unwrap();
    // Removing an already-absent path mirrors POSIX-unlink tolerance here.
    vfs.remove("/x").await.unwrap();
    assert!(vfs.open("/x", OpenMode::Read).await.is_err());
}

#[wasm_bindgen_test]
async fn create_or_open_never_truncates() {
    let vfs = fresh_vfs("conf-createoropen").await;
    let mut f = vfs.open("/x", OpenMode::CreateOrOpen).await.unwrap();
    f.write_at(0, b"keep me").await.unwrap();
    drop(f);
    let g = vfs.open("/x", OpenMode::CreateOrOpen).await.unwrap();
    assert_eq!(g.len().await.unwrap(), 7);
}

#[wasm_bindgen_test]
async fn read_write_mode_fails_when_absent() {
    let vfs = fresh_vfs("conf-rw-absent").await;
    let err = vfs.open("/nope", OpenMode::ReadWrite).await.err().unwrap();
    assert!(
        matches!(&err, PagedbError::Io(e) if e.kind() == std::io::ErrorKind::NotFound),
        "expected NotFound, got {err:?}"
    );
}
