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

fn driver_error(context: &str, error: impl std::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("{context}: {error:?}"))
}

fn require(condition: bool, message: &str) -> Result<(), JsValue> {
    if condition {
        Ok(())
    } else {
        Err(JsValue::from_str(message))
    }
}

/// Seeds the durable file image used by the file and namespace crash cases.
#[wasm_bindgen]
pub async fn idb_seed_baseline(root: String) -> Result<(), JsValue> {
    let vfs = IdbVfs::with_root(&root)
        .await
        .map_err(|error| driver_error("open IDB VFS", error))?;
    let mut baseline = vfs
        .open("baseline", OpenMode::CreateNew)
        .await
        .map_err(|error| driver_error("create baseline", error))?;
    baseline
        .write_at(0, b"durable")
        .await
        .map_err(|error| driver_error("write baseline", error))?;
    baseline
        .sync()
        .await
        .map_err(|error| driver_error("sync baseline", error))?;
    vfs.sync_dir("/")
        .await
        .map_err(|error| driver_error("publish baseline", error))
}

/// Seeds the durable database used by the header-write crash case.
#[wasm_bindgen]
pub async fn idb_seed_database(root: String) -> Result<(), JsValue> {
    let vfs = IdbVfs::with_root(&root)
        .await
        .map_err(|error| driver_error("open IDB VFS", error))?;
    let db = Db::open_internal(vfs, KEK, PAGE, REALM)
        .await
        .map_err(|error| driver_error("create baseline database", error))?;
    let mut write = db
        .begin_write()
        .await
        .map_err(|error| driver_error("begin baseline write", error))?;
    write
        .put(b"baseline", b"committed-gen-1")
        .await
        .map_err(|error| driver_error("write baseline value", error))?;
    write
        .commit()
        .await
        .map_err(|error| driver_error("commit baseline value", error))?;
    Ok(())
}

async fn verify_unpublished_path(root: &str, require_orphan_cleanup: bool) -> Result<(), JsValue> {
    let database = format!("pagedb-idb-vfs:{root}");
    let reopened = IdbVfs::with_root(root)
        .await
        .map_err(|error| driver_error("reopen IDB VFS", error))?;
    let baseline = reopened
        .open("baseline", OpenMode::Read)
        .await
        .map_err(|error| driver_error("open baseline", error))?;
    let mut bytes = [0; 7];
    let read = baseline
        .read_at(0, &mut bytes)
        .await
        .map_err(|error| driver_error("read baseline", error))?;
    require(read == 7 && &bytes == b"durable", "baseline file changed")?;
    require(
        reopened.open("doomed", OpenMode::Read).await.is_err(),
        "doomed path was published",
    )?;
    reopened
        .sync_dir("/")
        .await
        .map_err(|error| driver_error("sweep unpublished path", error))?;

    if require_orphan_cleanup {
        let store = IdbStore::open(&database)
            .await
            .map_err(|error| driver_error("open IDB store", error))?;
        let baseline_image = store
            .load_file(0)
            .await
            .map_err(|error| driver_error("load baseline image", error))?;
        let doomed_image = store
            .load_file(1)
            .await
            .map_err(|error| driver_error("load doomed image", error))?;
        require(
            baseline_image.as_deref() == Some(b"durable"),
            "baseline image changed",
        )?;
        require(
            doomed_image.is_none(),
            "orphaned doomed image survived sweep",
        )?;
        store.close();
    }

    drop(baseline);
    drop(reopened);
    IdbStore::delete(&database)
        .await
        .map_err(|error| driver_error("delete IDB store", error))
}

/// Verifies that a file-only sync remained unpublished and is swept on reopen.
#[wasm_bindgen]
pub async fn idb_verify_file_sync(root: String) -> Result<(), JsValue> {
    verify_unpublished_path(&root, true).await
}

/// Verifies that terminating inside the namespace transaction published nothing.
#[wasm_bindgen]
pub async fn idb_verify_namespace_put(root: String) -> Result<(), JsValue> {
    verify_unpublished_path(&root, true).await
}

/// Verifies that namespace sync made both the path and file image durable.
#[wasm_bindgen]
pub async fn idb_verify_namespace_sync(root: String) -> Result<(), JsValue> {
    let database = format!("pagedb-idb-vfs:{root}");
    let reopened = IdbVfs::with_root(&root)
        .await
        .map_err(|error| driver_error("reopen IDB VFS", error))?;
    let baseline = reopened
        .open("baseline", OpenMode::Read)
        .await
        .map_err(|error| driver_error("open baseline", error))?;
    let doomed = reopened
        .open("doomed", OpenMode::Read)
        .await
        .map_err(|error| driver_error("open published path", error))?;
    let mut baseline_bytes = [0; 7];
    let mut doomed_bytes = [0; 11];
    let baseline_read = baseline
        .read_at(0, &mut baseline_bytes)
        .await
        .map_err(|error| driver_error("read baseline", error))?;
    let doomed_read = doomed
        .read_at(0, &mut doomed_bytes)
        .await
        .map_err(|error| driver_error("read published path", error))?;
    require(
        baseline_read == 7 && &baseline_bytes == b"durable",
        "baseline file changed",
    )?;
    require(
        doomed_read == 11 && &doomed_bytes == b"unpublished",
        "published file image changed",
    )?;
    drop(doomed);
    drop(baseline);
    drop(reopened);
    IdbStore::delete(&database)
        .await
        .map_err(|error| driver_error("delete IDB store", error))
}

/// Verifies that a crash after the inactive header write kept the prior commit.
#[wasm_bindgen]
pub async fn idb_verify_header_write(root: String) -> Result<(), JsValue> {
    let database = format!("pagedb-idb-vfs:{root}");
    let vfs = IdbVfs::with_root(&root)
        .await
        .map_err(|error| driver_error("reopen IDB VFS", error))?;
    let db = Db::open_existing(vfs.clone(), KEK, PAGE, REALM)
        .await
        .map_err(|error| driver_error("reopen baseline database", error))?;
    let read = db
        .begin_read()
        .await
        .map_err(|error| driver_error("begin verification read", error))?;
    let baseline = read
        .get(b"baseline")
        .await
        .map_err(|error| driver_error("read baseline value", error))?;
    let victim = read
        .get(b"victim")
        .await
        .map_err(|error| driver_error("read victim value", error))?;
    require(
        baseline.as_deref() == Some(b"committed-gen-1"),
        "baseline commit changed",
    )?;
    require(victim.is_none(), "doomed commit became visible")?;
    drop(read);
    drop(db);
    drop(vfs);
    IdbStore::delete(&database)
        .await
        .map_err(|error| driver_error("delete IDB store", error))
}

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
