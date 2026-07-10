//! Real-worker Web Locks proof for the experimental, local-only `IdbVfs`.

#![cfg(all(target_arch = "wasm32", feature = "idb-vendor-spike"))]

use std::{cell::RefCell, rc::Rc};

use pagedb::errors::PagedbError;
use pagedb::vfs::{IdbStore, IdbVfs, Vfs};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const BOOTSTRAP: &str = r#"
self.onmessage = (event) => {
  const { lockName } = event.data;
  try {
    const held = new Promise(() => {});
    self.navigator.locks.request(
      lockName,
      { mode: 'exclusive', ifAvailable: true },
      (lock) => {
        if (lock === null) {
          postMessage('driver-error:worker lock unavailable');
          return undefined;
        }
        postMessage('idb-lock-held');
        return held;
      },
    ).catch((error) => {
      postMessage('driver-error:' + ((error && (error.message || String(error))) || 'unknown'));
    });
  } catch (error) {
    postMessage('driver-error:' + ((error && (error.message || String(error))) || 'unknown'));
  }
};
self.addEventListener('error', (event) => {
  postMessage('worker-onerror:' + ((event && event.message) || 'worker-error'));
});
"#;

async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let worker: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
        worker
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

fn spawn_lock_holder(lock_name: &str) -> (web_sys::Worker, Rc<RefCell<Vec<String>>>) {
    let source = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(BOOTSTRAP));
    let blob = web_sys::Blob::new_with_str_sequence(&source).unwrap();
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    let worker = web_sys::Worker::new(&url).expect("spawn nested IDB lock worker");

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

    let init = js_sys::Object::new();
    let set = |key: &str, value: &wasm_bindgen::JsValue| {
        js_sys::Reflect::set(&init, &wasm_bindgen::JsValue::from_str(key), value).unwrap();
    };
    set("lockName", &wasm_bindgen::JsValue::from_str(lock_name));
    worker.post_message(&init).unwrap();

    (worker, messages)
}

async fn wait_for_message(messages: &Rc<RefCell<Vec<String>>>) -> String {
    for _ in 0..200 {
        if let Some(message) = messages.borrow_mut().pop() {
            return message;
        }
        sleep_ms(50).await;
    }
    panic!("nested IDB lock worker sent no message within 10 seconds");
}

#[wasm_bindgen_test]
async fn idb_vfs_rejects_a_second_worker_then_releases_after_termination() {
    let root = format!("cross-worker-{}", js_sys::Date::now());
    let database = format!("pagedb-idb-vfs:{root}");
    let _ = IdbStore::delete(&database).await;

    let lock_name = format!("pagedb-idb-vfs:{root}:lock:/writer");
    let (worker, messages) = spawn_lock_holder(&lock_name);
    assert_eq!(wait_for_message(&messages).await, "idb-lock-held");

    let vfs = IdbVfs::with_root(&root).await.unwrap();
    assert!(matches!(
        vfs.lock_exclusive("/writer").await,
        Err(PagedbError::AlreadyLocked)
    ));

    worker.terminate();
    let mut recovered_lock = None;
    for _ in 0..100 {
        match vfs.lock_exclusive("writer").await {
            Ok(lock) => {
                recovered_lock = Some(lock);
                break;
            }
            Err(PagedbError::AlreadyLocked) => sleep_ms(50).await,
            Err(error) => panic!("unexpected lock error after worker termination: {error:?}"),
        }
    }
    assert!(
        recovered_lock.is_some(),
        "Web Lock was not released after worker termination"
    );

    drop(recovered_lock);
    drop(vfs);
    IdbStore::delete(&database).await.unwrap();
}
