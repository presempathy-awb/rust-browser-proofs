//! Browser execution test for the shipped OPFS capability-preflight module.

#![cfg(target_arch = "wasm32")]

use js_sys::Reflect;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const BOOTSTRAP_MODULE: &str = include_str!("../js/pagedb-opfs-bootstrap.mjs");

#[wasm_bindgen(inline_js = r#"
export async function loadCapabilityProbe(source) {
  const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
  try {
    const module = await import(url);
    return await module.probeOpfsCapabilities();
  } finally {
    URL.revokeObjectURL(url);
  }
}

export async function rejectsInvalidPersistenceOption(source) {
  const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
  try {
    const module = await import(url);
    await module.probeOpfsCapabilities({ requestPersistence: "yes" });
    return false;
  } catch (error) {
    return error instanceof TypeError;
  } finally {
    URL.revokeObjectURL(url);
  }
}
"#)]
extern "C" {
    fn loadCapabilityProbe(source: &str) -> js_sys::Promise;
    fn rejectsInvalidPersistenceOption(source: &str) -> js_sys::Promise;
}

fn field(value: &JsValue, name: &str) -> JsValue {
    Reflect::get(value, &JsValue::from_str(name)).unwrap()
}

fn bool_field(value: &JsValue, name: &str) -> bool {
    field(value, name).as_bool().unwrap()
}

#[wasm_bindgen_test]
async fn capability_probe_exercises_sync_access_handle_without_requesting_persistence() {
    let result = JsFuture::from(loadCapabilityProbe(BOOTSTRAP_MODULE))
        .await
        .unwrap();
    let opfs = field(&result, "opfs");
    let sync_access_handle = field(&result, "syncAccessHandle");
    let storage = field(&result, "storage");

    assert!(bool_field(&opfs, "available"));
    assert!(bool_field(&sync_access_handle, "available"));
    assert!(!bool_field(&storage, "persistenceRequested"));
    assert!(field(&storage, "usage").as_f64().is_some());
    assert!(field(&storage, "quota").as_f64().is_some());
    assert!(field(&result, "crossOriginIsolated").as_bool().is_some());
}

#[wasm_bindgen_test]
async fn capability_probe_rejects_non_boolean_persistence_requests() {
    assert!(
        JsFuture::from(rejectsInvalidPersistenceOption(BOOTSTRAP_MODULE))
            .await
            .unwrap()
            .as_bool()
            .unwrap()
    );
}
