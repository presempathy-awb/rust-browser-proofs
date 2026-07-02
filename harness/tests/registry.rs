//! Task 2 RED->GREEN: per-physical-file sync-access-handle registry.
//!
//! The observable contract under test:
//! - Two simultaneous opens of one file share ONE underlying handle (without
//!   dedupe the second `createSyncAccessHandle()` would throw
//!   `NoModificationAllowedError`).
//! - The handle closes synchronously when the LAST reference drops, so an
//!   immediate reopen never races a pending close.
//! - Browser failures surface as typed `PagedbError`s, never a wasm trap.

#![cfg(target_arch = "wasm32")]

mod support;

use pagedb::errors::PagedbError;
use pagedb::vfs::opfs::registry::FileRegistry;
use support::test_dir;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

#[wasm_bindgen_test]
async fn dedupe_two_opens_share_one_handle() {
    let dir = test_dir("reg-dedupe").await;
    let reg = FileRegistry::new();

    let a = reg.open(&dir, "f.bin", true, false).await.unwrap();
    // Without registry dedupe this second open would throw
    // NoModificationAllowedError (exclusive sync-access-handle lock).
    let b = reg.open(&dir, "f.bin", false, false).await.unwrap();

    a.write_at(0, b"abcd").unwrap();
    let mut buf = [0u8; 4];
    let n = b.read_at(0, &mut buf).unwrap();
    assert_eq!(n, 4);
    assert_eq!(&buf, b"abcd");

    drop(a);
    // Entry stays open through the surviving reference.
    b.write_at(4, b"ef").unwrap();
    assert_eq!(b.size().unwrap(), 6);
}

#[wasm_bindgen_test]
async fn last_drop_closes_synchronously_then_reopen_succeeds() {
    let dir = test_dir("reg-close").await;
    let reg = FileRegistry::new();

    {
        let f = reg.open(&dir, "g.bin", true, false).await.unwrap();
        f.write_at(0, b"persist").unwrap();
        f.flush().unwrap();
    } // last reference dropped -> handle must be closed HERE, synchronously

    // An immediate reopen must not hit a lingering exclusive lock.
    let g = reg.open(&dir, "g.bin", false, false).await.unwrap();
    let mut buf = [0u8; 7];
    let n = g.read_at(0, &mut buf).unwrap();
    assert_eq!(n, 7);
    assert_eq!(&buf, b"persist");
}

#[wasm_bindgen_test]
async fn create_new_fails_when_exists_even_while_open() {
    let dir = test_dir("reg-createnew").await;
    let reg = FileRegistry::new();

    let held = reg.open(&dir, "h.bin", true, false).await.unwrap();

    // While open (live map entry):
    let err = reg.open(&dir, "h.bin", true, true).await.err().unwrap();
    assert!(
        matches!(&err, PagedbError::Io(e) if e.kind() == std::io::ErrorKind::AlreadyExists),
        "expected AlreadyExists, got {err:?}"
    );

    drop(held);
    // And after close (probe via getFileHandle):
    let err = reg.open(&dir, "h.bin", true, true).await.err().unwrap();
    assert!(
        matches!(&err, PagedbError::Io(e) if e.kind() == std::io::ErrorKind::AlreadyExists),
        "expected AlreadyExists, got {err:?}"
    );
}

#[wasm_bindgen_test]
async fn missing_without_create_is_not_found() {
    let dir = test_dir("reg-notfound").await;
    let reg = FileRegistry::new();

    let err = reg
        .open(&dir, "nope.bin", false, false)
        .await
        .err()
        .unwrap();
    assert!(
        matches!(&err, PagedbError::Io(e) if e.kind() == std::io::ErrorKind::NotFound),
        "expected NotFound, got {err:?}"
    );
}

#[wasm_bindgen_test]
async fn short_read_at_eof_and_positioned_ops() {
    let dir = test_dir("reg-ops").await;
    let reg = FileRegistry::new();

    let f = reg.open(&dir, "ops.bin", true, false).await.unwrap();
    f.write_at(0, b"0123456789").unwrap();

    // Positioned read mid-file.
    let mut mid = [0u8; 4];
    assert_eq!(f.read_at(3, &mut mid).unwrap(), 4);
    assert_eq!(&mid, b"3456");

    // Short read at EOF.
    let mut tail = [0xffu8; 8];
    let n = f.read_at(6, &mut tail).unwrap();
    assert_eq!(n, 4);
    assert_eq!(&tail[..4], b"6789");

    // Truncate shrink + size.
    f.truncate(5).unwrap();
    assert_eq!(f.size().unwrap(), 5);
}

#[wasm_bindgen_test]
async fn quota_exhaustion_is_typed_not_a_trap() {
    let dir = test_dir("reg-quota").await;
    let reg = FileRegistry::new();

    let f = reg.open(&dir, "q.bin", true, false).await.unwrap();
    // 2^60 bytes exceeds any browser quota. Browsers differ in the exact
    // DOMException, so accept NoSpace (QuotaExceededError mapping) or a
    // typed Io error - the assertion is "typed error, no trap".
    match f.truncate(1u64 << 60) {
        Err(PagedbError::NoSpace) | Err(PagedbError::Io(_)) => {}
        Ok(()) => panic!("truncate to 2^60 unexpectedly succeeded"),
        Err(other) => panic!("unexpected error variant: {other:?}"),
    }
    // The handle must still be usable after the failed operation.
    f.write_at(0, b"ok").unwrap();
    assert_eq!(f.size().unwrap(), 2);
}

#[wasm_bindgen_test]
async fn writes_report_full_length() {
    let dir = test_dir("reg-writeall").await;
    let reg = FileRegistry::new();

    let f = reg.open(&dir, "w.bin", true, false).await.unwrap();
    let big = vec![0xa5u8; 256 * 1024];
    // write_at is write-all: anything less than the full buffer is a bug.
    assert_eq!(f.write_at(0, &big).unwrap(), big.len());
    f.flush().unwrap();
    assert_eq!(f.size().unwrap(), big.len() as u64);
}

#[wasm_bindgen_test]
async fn beyond_js_safe_integer_is_rejected_typed() {
    let dir = test_dir("reg-2p53").await;
    let reg = FileRegistry::new();

    let f = reg.open(&dir, "s.bin", true, false).await.unwrap();
    let over = 1u64 << 53; // MAX_SAFE_INTEGER + 1
    let werr = f.write_at(over, b"x").err().unwrap();
    assert!(
        matches!(&werr, PagedbError::Io(e) if e.kind() == std::io::ErrorKind::InvalidInput),
        "expected InvalidInput, got {werr:?}"
    );
    let terr = f.truncate(over).err().unwrap();
    assert!(
        matches!(&terr, PagedbError::Io(e) if e.kind() == std::io::ErrorKind::InvalidInput),
        "expected InvalidInput, got {terr:?}"
    );
    // Handle still healthy afterwards; durable write proves it.
    assert_eq!(f.write_at(0, b"ok").unwrap(), 2);
    f.flush().unwrap();
    assert_eq!(f.size().unwrap(), 2);
}
