//! Crash-oracle driver: runs inside the SACRIFICIAL worker.
//!
//! Exposed as a `#[wasm_bindgen]` export so the oracle bootstrap JS (see
//! `tests/oracle.rs`) can instantiate this same wasm module in a second
//! dedicated worker and invoke it. The driver seeds a database, then runs a
//! commit through a [`crate::fault::FaultVfs`] configured to PARK at the
//! requested phase - posting `phase-reached:...` to the owner, which then
//! calls `Worker.terminate()`. Real termination: no destructors, no closes,
//! abandoned sync access handles.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

use pagedb::vfs::opfs::OpfsVfs;
use pagedb::{Db, RealmId, SegmentKind, SegmentPageKind};

use crate::fault::{Action, FaultVfs, OpKind};

pub const PAGE: usize = 4096;
pub const KEK: [u8; 32] = [5u8; 32];
pub const REALM: RealmId = RealmId::new([2u8; 16]);

fn parse_kind(phase: &str) -> Option<(OpKind, u64)> {
    Some(match phase {
        "mid-vectored-write" => (OpKind::VectoredSubWrite, 1),
        "pages-written-pre-sync" => (OpKind::Sync, 1),
        "pages-synced-pre-header-write" => (OpKind::Write, 1),
        "header-written-pre-sync" => (OpKind::Sync, 2),
        "header-synced-pre-rename" => (OpKind::Rename, 1),
        "during-sync-dir" => (OpKind::SyncDirBefore, 1),
        "after-sync-dir-pre-gc" => (OpKind::SyncDirAfter, 1),
        _ => return None,
    })
}

/// Phase 0 (no parking): seed the database with the baseline commit the
/// oracle asserts against, plus a sealed staging segment ready to publish.
#[wasm_bindgen]
pub async fn oracle_seed(root: String) -> Result<(), JsValue> {
    let vfs = OpfsVfs::with_root(&root)
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    let db = Db::open_internal(vfs, KEK, PAGE, REALM)
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    let mut w = db
        .begin_write()
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    w.put(b"baseline", b"committed-gen-1")
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    w.commit()
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    Ok(())
}

/// Run the doomed commit: attempt to publish `victim=gen-2` plus a segment
/// promote through a FaultVfs parking at `phase`. The park posts the beacon;
/// the owner terminates this worker mid-flight.
#[wasm_bindgen]
pub async fn oracle_commit(root: String, phase: String) -> Result<(), JsValue> {
    let (kind, at) =
        parse_kind(&phase).ok_or_else(|| JsValue::from_str(&format!("unknown phase: {phase}")))?;
    let vfs = OpfsVfs::with_root(&root)
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    let vfs = FaultVfs::new_unarmed(vfs, kind, at, Action::Park);
    let fault_handle = vfs.clone();
    let db = Db::open_existing(vfs, KEK, PAGE, REALM)
        .await
        .map_err(|e| JsValue::from_str(&format!("seed-open: {e:?}")))?;

    // A segment so the doomed commit exercises the rename/publish path.
    let mut sw = db
        .create_segment(REALM, SegmentKind::Unspecified)
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    sw.append_page(SegmentPageKind::Data, b"doomed-segment-page")
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    let meta = sw
        .seal()
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;

    let mut w = db
        .begin_write()
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    w.put(b"victim", b"uncertain-gen-2")
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    w.link_segment("doomed.seg", &meta)
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    // Arm HERE: occurrence counts are commit-relative (open/seal used the
    // same VFS ops and must not consume the budget).
    fault_handle.arm();
    // The park happens INSIDE this commit; on the error-injection variants
    // it returns Err instead, which the driver reports upward.
    w.commit()
        .await
        .map_err(|e| JsValue::from_str(&format!("commit: {e:?}")))?;
    Ok(())
}
