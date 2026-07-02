//! Deterministic op-script + committed-state receipt.
//!
//! The same script runs natively (MemVfs reference) and in the browser
//! (OpfsVfs); both must produce the identical BLAKE3 receipt over the
//! committed KV values and segment page bytes. This is the native<->web
//! behavior-parity oracle (receipt equality means the engine observed the
//! same committed state through both backends).

use pagedb::vfs::traits::Vfs;
use pagedb::{Db, RealmId, SegmentKind, SegmentPageKind};

/// Pinned by tests/receipt_native.rs; asserted equal by the browser test.
pub const EXPECTED_RECEIPT: &str =
    "fe816e9d3945197858e644b5df53e570994efc9bba909d791571730dabc52a38";

pub const PAGE: usize = 4096;
pub const KEK: [u8; 32] = [7u8; 32];
pub const REALM: RealmId = RealmId::new([3u8; 16]);

/// Fixed key/value script: 40 puts across two commits, 10 overwrites, 5
/// deletes, one sealed+linked segment of 3 pages.
pub async fn run_script<V: Vfs + Clone>(db: &Db<V>) {
    let mut w = db.begin_write().await.unwrap();
    for i in 0..25u32 {
        w.put(
            format!("script-key-{i:03}").as_bytes(),
            format!("value-alpha-{i}").as_bytes(),
        )
        .await
        .unwrap();
    }
    w.commit().await.unwrap();

    let mut w = db.begin_write().await.unwrap();
    for i in 25..40u32 {
        w.put(
            format!("script-key-{i:03}").as_bytes(),
            format!("value-beta-{i}").as_bytes(),
        )
        .await
        .unwrap();
    }
    for i in 0..10u32 {
        w.put(
            format!("script-key-{i:03}").as_bytes(),
            format!("value-overwrite-{i}").as_bytes(),
        )
        .await
        .unwrap();
    }
    for i in 20..25u32 {
        w.delete(format!("script-key-{i:03}").as_bytes())
            .await
            .unwrap();
    }
    w.commit().await.unwrap();

    let mut sw = db
        .create_segment(REALM, SegmentKind::Unspecified)
        .await
        .unwrap();
    for p in 0..3u32 {
        sw.append_page(
            SegmentPageKind::Data,
            format!("segment-page-{p}").as_bytes(),
        )
        .await
        .unwrap();
    }
    sw.set_manifest(b"receipt-manifest").unwrap();
    let meta = sw.seal().await.unwrap();
    let mut t = db.begin_write().await.unwrap();
    t.link_segment("receipt.seg", &meta).await.unwrap();
    t.commit().await.unwrap();
}

/// BLAKE3 receipt over the script's committed observable state.
pub async fn compute_receipt<V: Vfs + Clone>(db: &Db<V>) -> String {
    let mut h = blake3::Hasher::new();
    let r = db.begin_read().await.unwrap();
    for i in 0..40u32 {
        let k = format!("script-key-{i:03}");
        h.update(k.as_bytes());
        match r.get(k.as_bytes()).await.unwrap() {
            Some(v) => {
                h.update(b"=");
                h.update(&v);
            }
            None => {
                h.update(b"<absent>");
            }
        }
        h.update(b"\n");
    }
    drop(r);
    let reader = db.open_segment(REALM, "receipt.seg").await.unwrap();
    for p in 1..=3u64 {
        h.update(&reader.read_page(p).await.unwrap());
    }
    h.finalize().to_hex().to_string()
}
