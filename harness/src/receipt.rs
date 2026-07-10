//! Deterministic op-script + committed-state receipt.
//!
//! The same script runs natively (MemVfs reference) and in the browser
//! (OpfsVfs); both must produce the identical BLAKE3 receipt over the
//! committed KV values and segment page bytes. This is the native<->web
//! behavior-parity oracle (receipt equality means the engine observed the
//! same committed state through both backends).

use pagedb::vfs::traits::Vfs;
use pagedb::{Db, RealmId, SegmentKind, SegmentPageKind};

/// Native reference receipts for every format-legal page size.
///
/// The receipt includes the sealed segment page count, so each format has a
/// distinct expected hash. Browser tests compare against this native matrix.
pub const RECEIPT_MATRIX: [(usize, &str); 5] = [
    (
        4096,
        "42649b2924cfa2c47d7dee7340f179e5718517660f50e9c40fdab46680cf7676",
    ),
    (
        8192,
        "aa72ecaf100edfbe183e2fb385801321658fd6e11b3e155fbd040ac4bdc8aff5",
    ),
    (
        16384,
        "5fe193170a154b40d4ed6a092e88f8145aec8aecd3b8b6e19b832baa7731108d",
    ),
    (
        32768,
        "8ee44b13e6e355b51e21abafb9a292c5dc1d5628ee0a2c29ae91bb07f6ff72d8",
    ),
    (
        65536,
        "25012effc8e0c757a5bf40ada4f5a06ac68a9d52a3fe4656cf36e3e6aee2cc54",
    ),
];
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

/// BLAKE3 receipt over the script's committed observable state: an ORDERED
/// prefix scan (count + every key/value pair - extra or missing keys and
/// ordering bugs change the hash), explicit absence probes for the deleted
/// keys, and the full segment (page count + every data page).
pub async fn compute_receipt<V: Vfs + Clone>(db: &Db<V>) -> String {
    let mut h = blake3::Hasher::new();
    let r = db.begin_read().await.unwrap();
    let pairs = r.scan(b"script-key-", b"script-key-\x7f").await.unwrap();
    h.update(&(pairs.len() as u64).to_le_bytes());
    for (k, v) in &pairs {
        h.update(k);
        h.update(b"=");
        h.update(v);
        h.update(b"\n");
    }
    // Deleted keys must be absent (scan omission alone could also mean a
    // range bug; probe them explicitly).
    for i in 20..25u32 {
        let k = format!("script-key-{i:03}");
        assert!(r.get(k.as_bytes()).await.unwrap().is_none(), "{k} undead");
        h.update(b"absent\n");
    }
    drop(r);
    let reader = db.open_segment(REALM, "receipt.seg").await.unwrap();
    h.update(&reader.meta().page_count.to_le_bytes());
    for p in 1..=3u64 {
        h.update(&reader.read_page(p).await.unwrap());
    }
    h.finalize().to_hex().to_string()
}
