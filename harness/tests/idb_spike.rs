//! IndexedDB viability spike for the future fallback adapter.
//!
//! This is intentionally not an `IdbVfs`: it proves the minimum browser
//! primitive that such a backend would need - an atomic, worker-side binary
//! transaction that can be read back after commit. PageDB still uses OPFS.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::JsValue;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

fn request_promise(request: &web_sys::IdbRequest) -> js_sys::Promise {
    let request_for_success = request.clone();
    let request_for_error = request.clone();

    js_sys::Promise::new(&mut move |resolve, reject| {
        let resolve = resolve.clone();
        let request_for_success = request_for_success.clone();
        let on_success = Closure::<dyn FnMut(web_sys::Event)>::once(move |_| {
            let result = request_for_success
                .result()
                .expect("successful IndexedDB request has a result");
            resolve
                .call1(&JsValue::UNDEFINED, &result)
                .expect("resolve IndexedDB request promise");
        });
        request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
        on_success.forget();

        let reject = reject.clone();
        let on_error = Closure::<dyn FnMut(web_sys::Event)>::once(move |_| {
            reject
                .call1(
                    &JsValue::UNDEFINED,
                    &JsValue::from_str("IndexedDB request failed"),
                )
                .expect("reject IndexedDB request promise");
        });
        request_for_error.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();
    })
}

async fn request_result(request: &web_sys::IdbRequest) -> JsValue {
    JsFuture::from(request_promise(request))
        .await
        .expect("IndexedDB request should succeed")
}

fn transaction_promise(transaction: &web_sys::IdbTransaction) -> js_sys::Promise {
    let transaction_for_complete = transaction.clone();
    let transaction_for_abort = transaction.clone();
    let transaction_for_error = transaction.clone();
    js_sys::Promise::new(&mut move |resolve, reject| {
        let resolve = resolve.clone();
        let on_complete = Closure::<dyn FnMut(web_sys::Event)>::once(move |_| {
            resolve
                .call0(&JsValue::UNDEFINED)
                .expect("resolve IndexedDB transaction promise");
        });
        transaction_for_complete.set_oncomplete(Some(on_complete.as_ref().unchecked_ref()));
        on_complete.forget();

        let reject_for_abort = reject.clone();
        let on_abort = Closure::<dyn FnMut(web_sys::Event)>::once(move |_| {
            reject_for_abort
                .call1(
                    &JsValue::UNDEFINED,
                    &JsValue::from_str("IndexedDB transaction aborted"),
                )
                .expect("reject IndexedDB transaction promise");
        });
        transaction_for_abort.set_onabort(Some(on_abort.as_ref().unchecked_ref()));
        on_abort.forget();

        let reject_for_error = reject.clone();
        let on_error = Closure::<dyn FnMut(web_sys::Event)>::once(move |_| {
            reject_for_error
                .call1(
                    &JsValue::UNDEFINED,
                    &JsValue::from_str("IndexedDB transaction failed"),
                )
                .expect("reject IndexedDB transaction promise");
        });
        transaction_for_error.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();
    })
}

async fn transaction_complete(transaction: &web_sys::IdbTransaction) {
    JsFuture::from(transaction_promise(transaction))
        .await
        .expect("IndexedDB transaction should commit");
}

#[wasm_bindgen_test]
async fn idb_readwrite_transaction_persists_two_binary_records() {
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let factory = global
        .indexed_db()
        .expect("read WorkerGlobalScope.indexedDB")
        .expect("IndexedDB must be available in a dedicated worker");
    let name = format!("pagedb-idb-viability-{}", js_sys::Date::now());

    let open = factory
        .open_with_u32(&name, 1)
        .expect("open IndexedDB database");
    let open_for_upgrade = open.clone();
    let on_upgrade = Closure::<dyn FnMut(web_sys::Event)>::wrap(Box::new(move |_| {
        let database: web_sys::IdbDatabase = open_for_upgrade
            .result()
            .expect("upgrade request result")
            .unchecked_into();
        database
            .create_object_store("files")
            .expect("create files object store");
    }));
    open.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));
    on_upgrade.forget();

    let database: web_sys::IdbDatabase =
        request_result(open.unchecked_ref()).await.unchecked_into();

    let write = database
        .transaction_with_str_and_mode("files", web_sys::IdbTransactionMode::Readwrite)
        .expect("open IndexedDB readwrite transaction");
    let store = write.object_store("files").expect("get files object store");
    let first = js_sys::Uint8Array::from(&[1u8, 3, 3, 7][..]);
    let second = js_sys::Uint8Array::from(&[9u8, 2, 6, 5][..]);
    store
        .put_with_key(first.as_ref(), &JsValue::from_str("first"))
        .expect("enqueue first record");
    store
        .put_with_key(second.as_ref(), &JsValue::from_str("second"))
        .expect("enqueue second record");
    transaction_complete(&write).await;

    let read = database
        .transaction_with_str_and_mode("files", web_sys::IdbTransactionMode::Readonly)
        .expect("open IndexedDB readonly transaction");
    let store = read.object_store("files").expect("get files object store");
    let first_read = store
        .get(&JsValue::from_str("first"))
        .expect("read first record");
    let second_read = store
        .get(&JsValue::from_str("second"))
        .expect("read second record");
    let first_read: js_sys::Uint8Array = request_result(&first_read).await.unchecked_into();
    let second_read: js_sys::Uint8Array = request_result(&second_read).await.unchecked_into();
    transaction_complete(&read).await;

    assert_eq!(first_read.to_vec(), vec![1, 3, 3, 7]);
    assert_eq!(second_read.to_vec(), vec![9, 2, 6, 5]);

    database.close();
    let delete = factory
        .delete_database(&name)
        .expect("delete test database");
    request_result(delete.unchecked_ref()).await;
}

#[wasm_bindgen_test]
async fn idb_abort_preserves_the_previous_committed_image() {
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let factory = global
        .indexed_db()
        .expect("read WorkerGlobalScope.indexedDB")
        .expect("IndexedDB must be available in a dedicated worker");
    let name = format!("pagedb-idb-abort-{}", js_sys::Date::now());

    let open = factory
        .open_with_u32(&name, 1)
        .expect("open IndexedDB database");
    let open_for_upgrade = open.clone();
    let on_upgrade = Closure::<dyn FnMut(web_sys::Event)>::wrap(Box::new(move |_| {
        let database: web_sys::IdbDatabase = open_for_upgrade
            .result()
            .expect("upgrade request result")
            .unchecked_into();
        database
            .create_object_store("files")
            .expect("create files object store");
    }));
    open.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));
    on_upgrade.forget();
    let database: web_sys::IdbDatabase =
        request_result(open.unchecked_ref()).await.unchecked_into();

    let committed = database
        .transaction_with_str_and_mode("files", web_sys::IdbTransactionMode::Readwrite)
        .expect("open initial transaction");
    let original = js_sys::Uint8Array::from(&[1u8, 2, 3][..]);
    committed
        .object_store("files")
        .expect("get files object store")
        .put_with_key(original.as_ref(), &JsValue::from_str("main"))
        .expect("enqueue original image");
    transaction_complete(&committed).await;

    let aborted = database
        .transaction_with_str_and_mode("files", web_sys::IdbTransactionMode::Readwrite)
        .expect("open replacement transaction");
    let replacement = js_sys::Uint8Array::from(&[9u8, 9, 9][..]);
    aborted
        .object_store("files")
        .expect("get files object store")
        .put_with_key(replacement.as_ref(), &JsValue::from_str("main"))
        .expect("enqueue replacement image");
    let aborted_wait = transaction_promise(&aborted);
    aborted.abort().expect("abort replacement transaction");
    assert!(
        JsFuture::from(aborted_wait).await.is_err(),
        "aborted IndexedDB transaction must reject"
    );

    let read = database
        .transaction_with_str_and_mode("files", web_sys::IdbTransactionMode::Readonly)
        .expect("open read transaction");
    let request = read
        .object_store("files")
        .expect("get files object store")
        .get(&JsValue::from_str("main"))
        .expect("read committed image");
    let image: js_sys::Uint8Array = request_result(&request).await.unchecked_into();
    transaction_complete(&read).await;
    assert_eq!(image.to_vec(), vec![1, 2, 3]);

    database.close();
    let delete = factory
        .delete_database(&name)
        .expect("delete test database");
    request_result(delete.unchecked_ref()).await;
}
