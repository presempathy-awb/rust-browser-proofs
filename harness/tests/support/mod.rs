//! Shared browser-test support: OPFS root access + per-test namespace dirs.
//!
//! Every test works under its own directory, pre-cleaned on entry, so a
//! panicked predecessor (which skips its own cleanup) can never poison a
//! rerun. wasm-pack spawns a fresh browser profile per invocation (OPFS
//! starts empty), so isolation only has to hold WITHIN one run - distinct
//! per-test names plus pre-clean deliver that.

#![cfg(target_arch = "wasm32")]
// Shared across test binaries; not every binary uses every helper.
#![allow(dead_code)]

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

pub async fn opfs_root() -> web_sys::FileSystemDirectoryHandle {
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    JsFuture::from(global.navigator().storage().get_directory())
        .await
        .expect("navigator.storage.getDirectory() failed - OPFS unavailable")
        .unchecked_into()
}

/// Fresh, isolated directory named `name` under the OPFS root. Removes any
/// leftover directory of the same name first (recursive), then creates it.
pub async fn test_dir(name: &str) -> web_sys::FileSystemDirectoryHandle {
    let root = opfs_root().await;
    let ropts = web_sys::FileSystemRemoveOptions::new();
    ropts.set_recursive(true);
    let _ = JsFuture::from(root.remove_entry_with_options(name, &ropts)).await;
    let dopts = web_sys::FileSystemGetDirectoryOptions::new();
    dopts.set_create(true);
    JsFuture::from(root.get_directory_handle_with_options(name, &dopts))
        .await
        .expect("create test dir")
        .unchecked_into()
}

/// Best-effort removal of a test directory (call at test end; pre-clean in
/// `test_dir` is the correctness guarantee, this just keeps the origin tidy).
pub async fn cleanup_dir(name: &str) {
    let root = opfs_root().await;
    let ropts = web_sys::FileSystemRemoveOptions::new();
    ropts.set_recursive(true);
    let _ = JsFuture::from(root.remove_entry_with_options(name, &ropts)).await;
}
