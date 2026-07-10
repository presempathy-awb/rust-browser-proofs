//! Real-worker termination proof for the IDB file-sync metadata boundary.

#![cfg(all(target_arch = "wasm32", feature = "idb-vendor-spike"))]

use std::{cell::RefCell, rc::Rc};

use pagedb::vfs::{IdbStore, IdbVfs, OpenMode, Vfs, VfsFile};
use pagedb::{Db, RealmId};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const BOOTSTRAP: &str = r#"
self.onmessage = async (event) => {
  const data = event.data;
  try {
    (0, eval)(data.glueJs + '\n;self.wasm_bindgen = wasm_bindgen;');
    await self.wasm_bindgen(data.wasmBytes);
    await self.wasm_bindgen[data.entrypoint](data.root);
    postMessage('driver-returned');
  } catch (error) {
    postMessage('driver-error:' + ((error && (error.message || error.stack || String(error))) || 'unknown'));
  }
};
"#;

const DRIVER_GLUE: &str = include_str!("../pkg-idb-driver/pagedb_opfs_harness.js");
const DRIVER_WASM: &[u8] = include_bytes!("../pkg-idb-driver/pagedb_opfs_harness_bg.wasm");
const PAGE: usize = 4096;
const KEK: [u8; 32] = [5u8; 32];
const REALM: RealmId = RealmId::new([2u8; 16]);

async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let scope: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
        scope
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

fn spawn_crash_worker(root: &str, entrypoint: &str) -> (web_sys::Worker, Rc<RefCell<Vec<String>>>) {
    let parts = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(BOOTSTRAP));
    let blob = web_sys::Blob::new_with_str_sequence(&parts).unwrap();
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    let worker = web_sys::Worker::new(&url).expect("nested crash worker");

    let messages = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&messages);
    let onmessage: Closure<dyn FnMut(web_sys::MessageEvent)> =
        Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            if let Some(message) = event.data().as_string() {
                sink.borrow_mut().push(message);
            }
        }));
    worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let input = js_sys::Object::new();
    let set = |key: &str, value: &wasm_bindgen::JsValue| {
        js_sys::Reflect::set(&input, &wasm_bindgen::JsValue::from_str(key), value).unwrap();
    };
    set("glueJs", &wasm_bindgen::JsValue::from_str(DRIVER_GLUE));
    let bytes = js_sys::Uint8Array::from(DRIVER_WASM);
    set("wasmBytes", &bytes.buffer().into());
    set("root", &wasm_bindgen::JsValue::from_str(root));
    set("entrypoint", &wasm_bindgen::JsValue::from_str(entrypoint));
    worker.post_message(&input).unwrap();
    (worker, messages)
}

async fn wait_for_message(messages: &Rc<RefCell<Vec<String>>>) -> String {
    for _ in 0..600 {
        if let Some(message) = messages.borrow_mut().pop() {
            return message;
        }
        sleep_ms(50).await;
    }
    panic!("crash driver did not reach its file-sync beacon within 30s");
}

async fn seed_baseline(root: &str) {
    let vfs = IdbVfs::with_root(root).await.unwrap();
    let mut baseline = vfs.open("baseline", OpenMode::CreateNew).await.unwrap();
    baseline.write_at(0, b"durable").await.unwrap();
    baseline.sync().await.unwrap();
    vfs.sync_dir("/").await.unwrap();
}

async fn seed_database(root: &str) {
    let vfs = IdbVfs::with_root(root).await.unwrap();
    let db = Db::open_internal(vfs, KEK, PAGE, REALM).await.unwrap();
    let mut write = db.begin_write().await.unwrap();
    write.put(b"baseline", b"committed-gen-1").await.unwrap();
    write.commit().await.unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_worker_termination_after_file_sync_keeps_metadata_unpublished() {
    let root = format!("crash-file-sync-{}", js_sys::Date::now());
    let database = format!("pagedb-idb-vfs:{root}");
    seed_baseline(&root).await;

    let (worker, messages) = spawn_crash_worker(&root, "idb_crash_after_file_sync");
    assert_eq!(wait_for_message(&messages).await, "idb-file-synced");
    worker.terminate();

    let reopened = IdbVfs::with_root(&root).await.unwrap();
    let baseline = reopened.open("baseline", OpenMode::Read).await.unwrap();
    let mut bytes = [0; 7];
    assert_eq!(baseline.read_at(0, &mut bytes).await.unwrap(), 7);
    assert_eq!(&bytes, b"durable");
    assert!(reopened.open("doomed", OpenMode::Read).await.is_err());
    reopened.sync_dir("/").await.unwrap();

    let store = IdbStore::open(&database).await.unwrap();
    assert_eq!(store.load_file(0).await.unwrap(), Some(b"durable".to_vec()));
    assert_eq!(store.load_file(1).await.unwrap(), None);
    store.close();
    drop(baseline);
    drop(reopened);
    IdbStore::delete(&database).await.unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_worker_termination_after_namespace_sync_keeps_metadata_published() {
    let root = format!("crash-namespace-sync-{}", js_sys::Date::now());
    let database = format!("pagedb-idb-vfs:{root}");
    seed_baseline(&root).await;

    let (worker, messages) = spawn_crash_worker(&root, "idb_crash_after_namespace_sync");
    assert_eq!(wait_for_message(&messages).await, "idb-namespace-synced");
    worker.terminate();

    let reopened = IdbVfs::with_root(&root).await.unwrap();
    let baseline = reopened.open("baseline", OpenMode::Read).await.unwrap();
    let doomed = reopened.open("doomed", OpenMode::Read).await.unwrap();
    let mut baseline_bytes = [0; 7];
    let mut doomed_bytes = [0; 11];
    assert_eq!(baseline.read_at(0, &mut baseline_bytes).await.unwrap(), 7);
    assert_eq!(doomed.read_at(0, &mut doomed_bytes).await.unwrap(), 11);
    assert_eq!(&baseline_bytes, b"durable");
    assert_eq!(&doomed_bytes, b"unpublished");

    drop(doomed);
    drop(baseline);
    drop(reopened);
    IdbStore::delete(&database).await.unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_worker_termination_after_header_write_recovers_the_prior_commit() {
    let root = format!("crash-header-sync-{}", js_sys::Date::now());
    let database = format!("pagedb-idb-vfs:{root}");
    seed_database(&root).await;

    let (worker, messages) = spawn_crash_worker(&root, "idb_crash_after_header_write");
    let beacon = wait_for_message(&messages).await;
    assert!(
        beacon.starts_with("phase-reached:Sync#2"),
        "unexpected crash beacon: {beacon}"
    );
    worker.terminate();

    let vfs = IdbVfs::with_root(&root).await.unwrap();
    let db = Db::open_existing(vfs.clone(), KEK, PAGE, REALM)
        .await
        .unwrap();
    let read = db.begin_read().await.unwrap();
    assert_eq!(
        read.get(b"baseline").await.unwrap().as_deref(),
        Some(b"committed-gen-1".as_ref())
    );
    assert_eq!(read.get(b"victim").await.unwrap(), None);
    drop(read);
    drop(db);
    drop(vfs);
    IdbStore::delete(&database).await.unwrap();
}

#[wasm_bindgen_test]
async fn idb_vfs_worker_termination_inside_namespace_transaction_keeps_path_unpublished() {
    let root = format!("crash-namespace-put-{}", js_sys::Date::now());
    let database = format!("pagedb-idb-vfs:{root}");
    seed_baseline(&root).await;

    let (worker, messages) = spawn_crash_worker(&root, "idb_crash_during_namespace_put");
    assert_eq!(
        wait_for_message(&messages).await,
        "idb-namespace-transaction-active"
    );
    worker.terminate();

    let reopened = IdbVfs::with_root(&root).await.unwrap();
    let baseline = reopened.open("baseline", OpenMode::Read).await.unwrap();
    let mut bytes = [0; 7];
    assert_eq!(baseline.read_at(0, &mut bytes).await.unwrap(), 7);
    assert_eq!(&bytes, b"durable");
    assert!(reopened.open("doomed", OpenMode::Read).await.is_err());
    reopened.sync_dir("/").await.unwrap();

    let store = IdbStore::open(&database).await.unwrap();
    assert_eq!(store.load_file(1).await.unwrap(), None);
    store.close();
    drop(baseline);
    drop(reopened);
    IdbStore::delete(&database).await.unwrap();
}
