//! Sacrificial-worker driver for the IDB file-sync crash boundary.
//!
//! The parent test terminates this worker after a file image reaches
//! IndexedDB but before `sync_dir()` can publish the path in the namespace.

#![cfg(all(target_arch = "wasm32", feature = "idb-crash-driver"))]

use pagedb::vfs::{IdbStore, IdbVfs, OpenMode, Vfs, VfsFile};
use pagedb::{Db, RealmId};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::fault::{Action, FaultVfs, OpKind};

const PAGE: usize = 4096;
const KEK: [u8; 32] = [5u8; 32];
const REALM: RealmId = RealmId::new([2u8; 16]);

/// Syncs a file image, optionally publishes its path, announces the crash cut, then parks.
async fn crash_after_file_sync(root: String, publish_namespace: bool) -> Result<(), JsValue> {
    let (beacon, namespace_state) = if publish_namespace {
        ("idb-namespace-synced", "publish doomed path")
    } else {
        ("idb-file-synced", "leave doomed path unpublished")
    };
    let vfs = IdbVfs::with_root(&root)
        .await
        .map_err(|error| JsValue::from_str(&format!("open IDB VFS: {error:?}")))?;
    let mut file = vfs
        .open("doomed", OpenMode::CreateNew)
        .await
        .map_err(|error| JsValue::from_str(&format!("create doomed file: {error:?}")))?;
    file.write_at(0, b"unpublished")
        .await
        .map_err(|error| JsValue::from_str(&format!("write doomed file: {error:?}")))?;
    file.sync()
        .await
        .map_err(|error| JsValue::from_str(&format!("sync doomed file: {error:?}")))?;

    if publish_namespace {
        vfs.sync_dir("/")
            .await
            .map_err(|error| JsValue::from_str(&format!("{namespace_state}: {error:?}")))?;
    }

    let scope: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    scope
        .post_message(&JsValue::from_str(beacon))
        .map_err(|error| JsValue::from_str(&format!("post crash beacon: {error:?}")))?;

    let never = js_sys::Promise::new(&mut |_, _| {});
    let _ = JsFuture::from(never).await;
    unreachable!("the parent must terminate the parked crash driver")
}

/// Syncs an unpublished file image, announces the precise crash cut, then parks.
#[wasm_bindgen]
pub async fn idb_crash_after_file_sync(root: String) -> Result<(), JsValue> {
    crash_after_file_sync(root, false).await
}

/// Publishes a file path, announces the precise crash cut, then parks.
#[wasm_bindgen]
pub async fn idb_crash_after_namespace_sync(root: String) -> Result<(), JsValue> {
    crash_after_file_sync(root, true).await
}

/// Parks after the new header is written but before its second commit-time sync.
#[wasm_bindgen]
pub async fn idb_crash_after_header_write(root: String) -> Result<(), JsValue> {
    let vfs = IdbVfs::with_root(&root)
        .await
        .map_err(|error| JsValue::from_str(&format!("open IDB VFS: {error:?}")))?;
    let vfs = FaultVfs::new_unarmed(vfs, OpKind::Sync, 2, Action::Park);
    let fault = vfs.clone();
    let db = Db::open_existing(vfs, KEK, PAGE, REALM)
        .await
        .map_err(|error| JsValue::from_str(&format!("open baseline database: {error:?}")))?;
    let mut write = db
        .begin_write()
        .await
        .map_err(|error| JsValue::from_str(&format!("begin doomed write: {error:?}")))?;
    write
        .put(b"victim", b"uncommitted-gen-2")
        .await
        .map_err(|error| JsValue::from_str(&format!("write doomed value: {error:?}")))?;
    fault.arm();
    write
        .commit()
        .await
        .map_err(|error| JsValue::from_str(&format!("doomed commit returned: {error:?}")))?;
    Ok(())
}

/// Parks inside an active namespace transaction after its `put()` is enqueued.
#[wasm_bindgen]
pub async fn idb_crash_during_namespace_put(root: String) -> Result<(), JsValue> {
    let vfs = IdbVfs::with_root(&root)
        .await
        .map_err(|error| JsValue::from_str(&format!("open IDB VFS: {error:?}")))?;
    let mut file = vfs
        .open("doomed", OpenMode::CreateNew)
        .await
        .map_err(|error| JsValue::from_str(&format!("create doomed file: {error:?}")))?;
    file.write_at(0, b"unpublished")
        .await
        .map_err(|error| JsValue::from_str(&format!("write doomed file: {error:?}")))?;
    file.sync()
        .await
        .map_err(|error| JsValue::from_str(&format!("sync doomed file: {error:?}")))?;
    IdbStore::pause_after_next_namespace_put_for_crash_test();
    vfs.sync_dir("/")
        .await
        .map_err(|error| JsValue::from_str(&format!("namespace sync returned: {error:?}")))?;
    Ok(())
}
