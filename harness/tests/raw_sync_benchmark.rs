//! Browser contract test for the raw OPFS sync-handle benchmark baseline.

#![cfg(target_arch = "wasm32")]

use js_sys::Reflect;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const BENCHMARK_MODULE: &str = include_str!("../js/pagedb-opfs-benchmark.mjs");

#[wasm_bindgen(inline_js = r#"
export async function runRawSyncBenchmark(source) {
  const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
  try {
    const module = await import(url);
    return await module.benchmarkRawSyncAccessHandle({ byteLength: 4096, iterations: 3 });
  } finally {
    URL.revokeObjectURL(url);
  }
}

export async function rejectsInvalidRawSyncBenchmark(source) {
  const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
  try {
    const module = await import(url);
    await module.benchmarkRawSyncAccessHandle({ byteLength: 0, iterations: 1 });
    return false;
  } catch (error) {
    return error instanceof RangeError;
  } finally {
    URL.revokeObjectURL(url);
  }
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = runRawSyncBenchmark)]
    fn run_raw_sync_benchmark(source: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = rejectsInvalidRawSyncBenchmark)]
    fn rejects_invalid_raw_sync_benchmark(source: &str) -> js_sys::Promise;
}

fn field(value: &JsValue, name: &str) -> JsValue {
    Reflect::get(value, &JsValue::from_str(name)).unwrap()
}

fn number_field(value: &JsValue, name: &str) -> f64 {
    field(value, name).as_f64().unwrap()
}

#[wasm_bindgen_test]
async fn raw_sync_benchmark_reports_completed_read_and_write_work() {
    let result = JsFuture::from(run_raw_sync_benchmark(BENCHMARK_MODULE))
        .await
        .unwrap();
    let writes = field(&result, "writes");
    let reads = field(&result, "reads");

    assert_eq!(number_field(&result, "byteLength"), 4096.0);
    assert_eq!(number_field(&result, "iterations"), 3.0);
    assert_eq!(number_field(&writes, "bytes"), 12_288.0);
    assert_eq!(number_field(&reads, "bytes"), 12_288.0);
    assert_eq!(number_field(&reads, "checksum"), 79.0);
    assert!(number_field(&writes, "elapsedMs").is_finite());
    assert!(number_field(&reads, "elapsedMs").is_finite());
}

#[wasm_bindgen_test]
async fn raw_sync_benchmark_rejects_invalid_workload_dimensions() {
    assert!(
        JsFuture::from(rejects_invalid_raw_sync_benchmark(BENCHMARK_MODULE))
            .await
            .unwrap()
            .as_bool()
            .unwrap()
    );
}
