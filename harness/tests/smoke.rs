//! Task 1 smoke test: prove the test vehicle before any backend work.
//!
//! Runs inside a DEDICATED WORKER (`run_in_dedicated_worker`) - the only
//! context where `createSyncAccessHandle()` exists - and exercises a raw
//! `FileSystemSyncAccessHandle` write/read/close/remove round-trip through
//! web-sys, with no pagedb involvement.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

async fn opfs_root() -> web_sys::FileSystemDirectoryHandle {
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let storage = global.navigator().storage();
    JsFuture::from(storage.get_directory())
        .await
        .expect("navigator.storage.getDirectory() failed - OPFS unavailable")
        .unchecked_into()
}

#[wasm_bindgen_test]
async fn sync_access_handle_round_trip_in_dedicated_worker() {
    let root = opfs_root().await;

    let opts = web_sys::FileSystemGetFileOptions::new();
    opts.set_create(true);
    let fh: web_sys::FileSystemFileHandle =
        JsFuture::from(root.get_file_handle_with_options("smoke.bin", &opts))
            .await
            .expect("getFileHandle(create) failed")
            .unchecked_into();

    let sah: web_sys::FileSystemSyncAccessHandle = JsFuture::from(fh.create_sync_access_handle())
        .await
        .expect("createSyncAccessHandle() failed - not in a dedicated worker?")
        .unchecked_into();

    let data = [7u8, 1, 2, 3];
    let written = sah.write_with_u8_array(&data).expect("sync write failed");
    assert_eq!(written, 4.0);
    sah.flush().expect("flush failed");

    let mut buf = [0u8; 4];
    let read_opts = web_sys::FileSystemReadWriteOptions::new();
    read_opts.set_at(0.0);
    let read = sah
        .read_with_u8_array_and_options(&mut buf, &read_opts)
        .expect("sync read failed");
    assert_eq!(read, 4.0);
    assert_eq!(buf, data);

    let size = sah.get_size().expect("getSize failed");
    assert_eq!(size, 4.0);

    sah.close();

    JsFuture::from(root.remove_entry("smoke.bin"))
        .await
        .expect("removeEntry cleanup failed");
}
