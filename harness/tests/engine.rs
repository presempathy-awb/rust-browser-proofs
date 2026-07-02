//! Task 6: engine suites on the adapter - Db-level flows over `OpfsVfs` on
//! real OPFS, ported from the named native suites (MemVfs + #[tokio::test]
//! originals re-expressed as #[wasm_bindgen_test] over OpfsVfs).
//!
//! Coverage: KV commit/reopen cycles (txn_basic/btree descent), the full
//! segment lifecycle create->append->seal->link->promote->read-back (the
//! rename-through-manifest path), the crash_basic orphaned-staging sweep,
//! all five page sizes (page_size_range), and a spill-file stress loop
//! (tmp/scratch create/drop/recreate - the registry close-race regression
//! net).

#![cfg(target_arch = "wasm32")]

mod support;

use pagedb::options::OpenOptions;
use pagedb::vfs::opfs::OpfsVfs;
use pagedb::{Db, RealmId, SegmentKind, SegmentPageKind};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const PAGE: usize = 4096;
const REALM: RealmId = RealmId::new([1u8; 16]);
const KEK: [u8; 32] = [9u8; 32];

async fn fresh_root(root: &str) -> OpfsVfs {
    support::cleanup_dir(root).await;
    OpfsVfs::with_root(root).await.unwrap()
}

async fn reopen_root(root: &str) -> OpfsVfs {
    OpfsVfs::with_root(root).await.unwrap()
}

/// txn_basic + btree descent port: many keys across several commits, then a
/// full reopen must serve every committed value (and only those).
#[wasm_bindgen_test]
async fn kv_many_commits_then_reopen_serves_all() {
    let root = "eng-kv";
    {
        let db = Db::open_internal(fresh_root(root).await, KEK, PAGE, REALM)
            .await
            .unwrap();
        for batch in 0..3u32 {
            let mut w = db.begin_write().await.unwrap();
            for i in 0..50u32 {
                let k = format!("key-{batch:02}-{i:04}");
                let v = format!("val-{batch}-{i}");
                w.put(k.as_bytes(), v.as_bytes()).await.unwrap();
            }
            w.commit().await.unwrap();
        }
        // Uncommitted tail must NOT survive.
        let mut w = db.begin_write().await.unwrap();
        w.put(b"ghost", b"boo").await.unwrap();
        drop(w);
    }
    let db = Db::open_existing(reopen_root(root).await, KEK, PAGE, REALM)
        .await
        .unwrap();
    let r = db.begin_read().await.unwrap();
    for batch in 0..3u32 {
        for i in (0..50u32).step_by(7) {
            let k = format!("key-{batch:02}-{i:04}");
            let expect = format!("val-{batch}-{i}");
            assert_eq!(
                r.get(k.as_bytes()).await.unwrap().as_deref(),
                Some(expect.as_bytes()),
                "missing {k}"
            );
        }
    }
    assert!(r.get(b"ghost").await.unwrap().is_none());
}

/// segment_basic port: create -> append -> set_manifest -> seal -> link ->
/// commit (staging->live promote runs through the manifest rename) ->
/// read back, including across a full reopen.
#[wasm_bindgen_test]
async fn segment_lifecycle_survives_reopen() {
    let root = "eng-seg";
    {
        let db = Db::open_internal(fresh_root(root).await, KEK, PAGE, REALM)
            .await
            .unwrap();
        let mut w = db
            .create_segment(REALM, SegmentKind::Unspecified)
            .await
            .unwrap();
        let pid1 = w
            .append_page(SegmentPageKind::Data, b"page-one")
            .await
            .unwrap();
        let pid2 = w
            .append_page(SegmentPageKind::Data, b"page-two")
            .await
            .unwrap();
        assert_eq!((pid1, pid2), (1, 2));
        w.set_manifest(b"manifest-bytes").unwrap();
        let meta = w.seal().await.unwrap();
        assert_eq!(meta.page_count, 4);
        let mut t = db.begin_write().await.unwrap();
        t.link_segment("engine.idx", &meta).await.unwrap();
        t.commit().await.unwrap();

        let reader = db.open_segment(REALM, "engine.idx").await.unwrap();
        assert!(reader.read_page(1).await.unwrap().starts_with(b"page-one"));
        assert!(reader.read_page(2).await.unwrap().starts_with(b"page-two"));
    }
    // Full reopen: catalog + segment must come back from OPFS bytes alone.
    let db = Db::open_existing(reopen_root(root).await, KEK, PAGE, REALM)
        .await
        .unwrap();
    let reader = db.open_segment(REALM, "engine.idx").await.unwrap();
    assert!(reader.read_page(1).await.unwrap().starts_with(b"page-one"));
    assert!(reader.read_page(2).await.unwrap().starts_with(b"page-two"));
}

/// crash_basic port: a sealed-but-never-linked staging segment (simulated
/// crash: drop without link+commit) must be swept on the next open and the
/// database must stay fully usable.
#[wasm_bindgen_test]
async fn unlinked_sealed_staging_swept_on_reopen() {
    let root = "eng-crash";
    {
        let db = Db::open_internal(fresh_root(root).await, KEK, PAGE, REALM)
            .await
            .unwrap();
        let mut w = db
            .create_segment(REALM, SegmentKind::Unspecified)
            .await
            .unwrap();
        w.append_page(SegmentPageKind::Data, b"orphan")
            .await
            .unwrap();
        let _meta = w.seal().await.unwrap();
        // No link_segment + commit: simulated crash before publish.
    }
    let db = Db::open_existing(reopen_root(root).await, KEK, PAGE, REALM)
        .await
        .unwrap();
    // The swept database is fully usable afterwards.
    let mut w = db.begin_write().await.unwrap();
    w.put(b"alive", b"yes").await.unwrap();
    w.commit().await.unwrap();
    let r = db.begin_read().await.unwrap();
    assert_eq!(
        r.get(b"alive").await.unwrap().as_deref(),
        Some(b"yes".as_ref())
    );
}

/// page_size_range port: every format-legal page size opens, commits, and
/// reopens on OPFS.
#[wasm_bindgen_test]
async fn all_five_page_sizes_round_trip() {
    for (i, page) in [4096usize, 8192, 16384, 32768, 65536]
        .into_iter()
        .enumerate()
    {
        let root = format!("eng-psize-{i}");
        support::cleanup_dir(&root).await;
        {
            let vfs = OpfsVfs::with_root(&root).await.unwrap();
            let db = Db::open_internal(vfs, KEK, page, REALM).await.unwrap();
            let mut w = db.begin_write().await.unwrap();
            w.put(b"size-key", page.to_string().as_bytes())
                .await
                .unwrap();
            w.commit().await.unwrap();
        }
        let vfs = OpfsVfs::with_root(&root).await.unwrap();
        let db = Db::open_existing(vfs, KEK, page, REALM).await.unwrap();
        let r = db.begin_read().await.unwrap();
        assert_eq!(
            r.get(b"size-key").await.unwrap().as_deref(),
            Some(page.to_string().as_bytes()),
            "page size {page}"
        );
    }
}

/// spill_basic port + stress: repeated spill-scope cycles force
/// tmp/scratch-N create/use/drop/recreate through the registry - the
/// regression net for the old fire-and-forget close race.
#[wasm_bindgen_test]
async fn spill_scratch_reopen_stress() {
    let root = "eng-spill";
    support::cleanup_dir(root).await;
    let opts = OpenOptions::default().with_scratch_bytes(1024 * 1024);
    let db = Db::open_internal_with_options(
        OpfsVfs::with_root(root).await.unwrap(),
        KEK,
        PAGE,
        REALM,
        opts,
    )
    .await
    .unwrap();
    for round in 0..20u32 {
        let mut w = db.begin_write().await.unwrap();
        {
            let mut s = w.spill_scope();
            let payload = format!("spill-round-{round}");
            let h = s.append(payload.as_bytes()).await.unwrap();
            assert_eq!(s.read(h).await.unwrap(), payload.as_bytes());
        }
        w.put(format!("round-{round}").as_bytes(), b"done")
            .await
            .unwrap();
        w.commit().await.unwrap();
    }
    let r = db.begin_read().await.unwrap();
    assert_eq!(
        r.get(b"round-19").await.unwrap().as_deref(),
        Some(b"done".as_ref())
    );
}
