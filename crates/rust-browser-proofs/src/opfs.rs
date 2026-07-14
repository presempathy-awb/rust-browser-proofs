//! Engine-independent OPFS sync-access-handle proof helpers.
//!
//! These helpers deliberately exercise only browser storage primitives. A
//! database or VFS must provide a separate adapter before it can claim
//! backend-specific conformance or crash recovery.

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

const ROUND_TRIP_BYTES: [u8; 4] = [7, 1, 2, 3];
const DEFAULT_BENCHMARK_BYTES: usize = 4_096;
const DEFAULT_BENCHMARK_ITERATIONS: u32 = 3;

/// Completed raw OPFS work for a small browser-health baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawSyncBenchmark {
    /// Bytes written across all iterations, including each flushed rewrite.
    pub written_bytes: u64,
    /// Bytes read across all iterations.
    pub read_bytes: u64,
    /// XOR of the first and last byte for each completed read.
    pub checksum: u8,
    /// Wall-clock write duration reported by the worker's JavaScript clock.
    pub write_elapsed_ms: f64,
    /// Wall-clock read duration reported by the worker's JavaScript clock.
    pub read_elapsed_ms: f64,
}

/// Prove one dedicated-worker OPFS write, flush, close, reopen, and read.
pub async fn assert_sync_access_handle_round_trip() -> Result<(), JsValue> {
    let (root, dir_name, directory) = fresh_test_directory("round-trip").await?;
    let result = async {
        let file = create_file(&directory, "payload.bin").await?;
        let handle = create_sync_access_handle(&file).await?;

        let write = handle.write_with_u8_array(&ROUND_TRIP_BYTES)?;
        if write != ROUND_TRIP_BYTES.len() as f64 {
            handle.close();
            return Err(contract_error(
                "sync-handle write was shorter than the payload",
            ));
        }
        handle.flush()?;
        handle.close();

        let reopened = create_sync_access_handle(&file).await?;
        let mut bytes = [0_u8; ROUND_TRIP_BYTES.len()];
        let read_options = read_write_options(0.0);
        let read = reopened.read_with_u8_array_and_options(&mut bytes, &read_options)?;
        let size = reopened.get_size()?;
        reopened.close();

        if read != ROUND_TRIP_BYTES.len() as f64 {
            return Err(contract_error(
                "sync-handle reopen read was shorter than the payload",
            ));
        }
        if bytes != ROUND_TRIP_BYTES {
            return Err(contract_error(
                "sync-handle reopen returned different bytes",
            ));
        }
        if size != ROUND_TRIP_BYTES.len() as f64 {
            return Err(contract_error("sync-handle size did not survive reopen"));
        }
        Ok(())
    }
    .await;

    cleanup_test_directory(&root, &dir_name).await;
    result
}

/// Measure repeated raw sync-handle writes with `flush()` and rereads.
pub async fn benchmark_raw_sync_access_handle(
    byte_length: usize,
    iterations: u32,
) -> Result<RawSyncBenchmark, JsValue> {
    validate_workload(byte_length, iterations)?;

    let (root, dir_name, directory) = fresh_test_directory("benchmark").await?;
    let result = async {
        let file = create_file(&directory, "payload.bin").await?;
        let handle = create_sync_access_handle(&file).await?;
        let write_buffer: Vec<u8> = (0..byte_length).map(|index| (index % 251) as u8).collect();
        let mut read_buffer = vec![0_u8; byte_length];
        let read_write_options = read_write_options(0.0);

        let write_started = js_sys::Date::now();
        let mut written_bytes = 0_u64;
        for _ in 0..iterations {
            let written =
                handle.write_with_u8_array_and_options(&write_buffer, &read_write_options)?;
            if written != byte_length as f64 {
                handle.close();
                return Err(contract_error(
                    "sync-handle benchmark write was shorter than the payload",
                ));
            }
            handle.flush()?;
            written_bytes += written as u64;
        }
        let write_elapsed_ms = js_sys::Date::now() - write_started;

        let read_started = js_sys::Date::now();
        let mut read_bytes = 0_u64;
        let mut checksum = 0_u8;
        for _ in 0..iterations {
            let read =
                handle.read_with_u8_array_and_options(&mut read_buffer, &read_write_options)?;
            if read != byte_length as f64 {
                handle.close();
                return Err(contract_error(
                    "sync-handle benchmark read was shorter than the payload",
                ));
            }
            read_bytes += read as u64;
            checksum ^= read_buffer[0] ^ read_buffer[byte_length - 1];
        }
        let read_elapsed_ms = js_sys::Date::now() - read_started;
        handle.close();

        Ok(RawSyncBenchmark {
            written_bytes,
            read_bytes,
            checksum,
            write_elapsed_ms,
            read_elapsed_ms,
        })
    }
    .await;

    cleanup_test_directory(&root, &dir_name).await;
    result
}

/// Run the default small workload and validate that all requested I/O finished.
pub async fn assert_raw_sync_baseline() -> Result<(), JsValue> {
    let result =
        benchmark_raw_sync_access_handle(DEFAULT_BENCHMARK_BYTES, DEFAULT_BENCHMARK_ITERATIONS)
            .await?;
    let expected_bytes = (DEFAULT_BENCHMARK_BYTES as u64) * u64::from(DEFAULT_BENCHMARK_ITERATIONS);
    let expected_checksum = 79;

    if result.written_bytes != expected_bytes || result.read_bytes != expected_bytes {
        return Err(contract_error(
            "raw sync baseline did not complete the requested I/O",
        ));
    }
    if result.checksum != expected_checksum {
        return Err(contract_error(
            "raw sync baseline read back an unexpected payload",
        ));
    }
    if !result.write_elapsed_ms.is_finite() || !result.read_elapsed_ms.is_finite() {
        return Err(contract_error(
            "raw sync baseline did not report finite elapsed time",
        ));
    }
    Ok(())
}

fn validate_workload(byte_length: usize, iterations: u32) -> Result<(), JsValue> {
    if byte_length == 0 {
        return Err(contract_error("byte_length must be positive"));
    }
    if iterations == 0 {
        return Err(contract_error("iterations must be positive"));
    }
    byte_length
        .checked_mul(iterations as usize)
        .ok_or_else(|| contract_error("byte_length * iterations overflowed usize"))?;
    Ok(())
}

async fn fresh_test_directory(
    prefix: &str,
) -> Result<
    (
        web_sys::FileSystemDirectoryHandle,
        String,
        web_sys::FileSystemDirectoryHandle,
    ),
    JsValue,
> {
    let global: web_sys::WorkerGlobalScope = js_sys::global().dyn_into()?;
    let root: web_sys::FileSystemDirectoryHandle =
        JsFuture::from(global.navigator().storage().get_directory())
            .await?
            .dyn_into()?;
    let name = format!(".rust-browser-proofs-{prefix}-{}", js_sys::Math::random());
    let options = web_sys::FileSystemGetDirectoryOptions::new();
    options.set_create(true);
    let directory = JsFuture::from(root.get_directory_handle_with_options(&name, &options))
        .await?
        .dyn_into()?;
    Ok((root, name, directory))
}

async fn create_file(
    directory: &web_sys::FileSystemDirectoryHandle,
    name: &str,
) -> Result<web_sys::FileSystemFileHandle, JsValue> {
    let options = web_sys::FileSystemGetFileOptions::new();
    options.set_create(true);
    JsFuture::from(directory.get_file_handle_with_options(name, &options))
        .await?
        .dyn_into()
}

async fn create_sync_access_handle(
    file: &web_sys::FileSystemFileHandle,
) -> Result<web_sys::FileSystemSyncAccessHandle, JsValue> {
    JsFuture::from(file.create_sync_access_handle())
        .await?
        .dyn_into()
}

fn read_write_options(at: f64) -> web_sys::FileSystemReadWriteOptions {
    let options = web_sys::FileSystemReadWriteOptions::new();
    options.set_at(at);
    options
}

async fn cleanup_test_directory(root: &web_sys::FileSystemDirectoryHandle, name: &str) {
    let options = web_sys::FileSystemRemoveOptions::new();
    options.set_recursive(true);
    let _ = JsFuture::from(root.remove_entry_with_options(name, &options)).await;
}

fn contract_error(message: &str) -> JsValue {
    JsValue::from_str(message)
}
