//! Task 7: the crash & durability oracle.
//!
//! REAL worker termination at every instrumented commit-phase beacon: a
//! sacrificial dedicated worker instantiates this same wasm module (via the
//! wasm-bindgen-test no-modules glue) and runs `driver::oracle_commit`
//! through a parking `FaultVfs`. At the target phase the driver posts a
//! beacon and stalls; the test terminates the worker - no destructors, no
//! closes, abandoned sync access handles - then reopens with bounded retry
//! and asserts the phase group's expectation:
//!
//! - PRE-PUBLICATION (mid-vectored-write, pages-written-pre-sync,
//!   pages-synced-pre-header-write): exactly the OLD state.
//! - PUBLICATION-AMBIGUOUS (header-written-pre-sync): OLD XOR NEW, atomic
//!   either way, never torn.
//! - POST-PUBLICATION (header-synced-pre-rename, during-sync-dir,
//!   after-sync-dir-pre-gc): exactly the NEW state - the first two prove
//!   open-flow reconcile (catalog references the segment while its live
//!   manifest path is missing; staging must be promoted).
//!
//! Plus FaultVfs error-injection cases (typed error, clean abort, reopen
//! serves the old state) and the manifest/data-mismatch case.

#![cfg(target_arch = "wasm32")]

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use pagedb::errors::PagedbError;
use pagedb::vfs::opfs::OpfsVfs;
use pagedb::{Db, RealmId};
use pagedb_opfs_harness::driver::{self, KEK, PAGE};
use pagedb_opfs_harness::fault::{Action, FaultVfs, OpKind};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const REALM: RealmId = driver::REALM;

async fn sleep_ms(ms: i32) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        let g: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
        g.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    });
    let _ = JsFuture::from(p).await;
}

/// The sacrificial worker's bootstrap. The driver is a SELF-CONTAINED
/// wasm-bindgen `no-modules` bundle built from this harness lib (see the
/// justfile `build-driver` recipe) and embedded in the test binary - no
/// assumptions about what the test server serves. The glue JS is eval'd
/// (defines the global `wasm_bindgen`), initialised from the shipped bytes,
/// and the doomed commit runs until its park beacon.
const BOOTSTRAP: &str = r#"
self.onmessage = async (e) => {
  const d = e.data;
  try {
    // The no-modules glue declares `let wasm_bindgen` - capture it onto
    // self within the same eval scope so it survives.
    (0, eval)(d.glueJs + '\n;self.wasm_bindgen = wasm_bindgen;');
    await self.wasm_bindgen(d.wasmBytes);
    await self.wasm_bindgen.oracle_commit(d.root, d.phase);
    postMessage('commit-returned');
  } catch (err) {
    postMessage('driver-error:' + ((err && (err.message || err.stack || String(err))) || 'unknown'));
  }
};
self.addEventListener('error', (e) => {
  postMessage('driver-error:' + ((e && e.message) || 'worker-error'));
});
"#;

/// Driver bundle, embedded at compile time (built by `just build-driver`).
const DRIVER_GLUE: &str = include_str!("../pkg-driver/pagedb_opfs_harness.js");
const DRIVER_WASM: &[u8] = include_bytes!("../pkg-driver/pagedb_opfs_harness_bg.wasm");

fn spawn_oracle_worker(root: &str, phase: &str) -> (web_sys::Worker, Rc<RefCell<Vec<String>>>) {
    let parts = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(BOOTSTRAP));
    let blob = web_sys::Blob::new_with_str_sequence(&parts).unwrap();
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    let worker = web_sys::Worker::new(&url).expect("nested worker");

    let msgs: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&msgs);
    let onmessage: Closure<dyn FnMut(web_sys::MessageEvent)> =
        Closure::wrap(Box::new(move |ev: web_sys::MessageEvent| {
            if let Some(s) = ev.data().as_string() {
                sink.borrow_mut().push(s);
            }
        }));
    worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget(); // lives as long as the worker
    let sink2 = Rc::clone(&msgs);
    // Generic Event: a failed worker may fire a bare Event without
    // ErrorEvent fields; read `message` defensively via Reflect.
    let onerror: Closure<dyn FnMut(web_sys::Event)> =
        Closure::wrap(Box::new(move |ev: web_sys::Event| {
            let msg = js_sys::Reflect::get(ev.as_ref(), &"message".into())
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_else(|| "worker failed to start".to_string());
            sink2.borrow_mut().push(format!("worker-onerror:{msg}"));
        }));
    worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    let init = js_sys::Object::new();
    let set = |k: &str, v: &wasm_bindgen::JsValue| {
        js_sys::Reflect::set(&init, &wasm_bindgen::JsValue::from_str(k), v).unwrap();
    };
    set("glueJs", &wasm_bindgen::JsValue::from_str(DRIVER_GLUE));
    let bytes = js_sys::Uint8Array::from(DRIVER_WASM);
    set("wasmBytes", &bytes.buffer().into());
    set("root", &wasm_bindgen::JsValue::from_str(root));
    set("phase", &wasm_bindgen::JsValue::from_str(phase));
    worker.post_message(&init).unwrap();
    (worker, msgs)
}

/// Reopen with bounded retry: sync-access-handle locks release
/// asynchronously after Worker.terminate().
async fn reopen_with_retry(root: &str) -> Db<OpfsVfs> {
    for attempt in 0..100u32 {
        let vfs = match OpfsVfs::with_root(root).await {
            Ok(v) => v,
            Err(PagedbError::AlreadyLocked) if attempt < 99 => {
                sleep_ms(100).await;
                continue;
            }
            Err(e) => panic!("vfs reopen failed: {e:?}"),
        };
        match Db::open_existing(vfs, KEK, PAGE, REALM).await {
            Ok(db) => return db,
            // ONLY lock-release latency is retryable (a terminated worker's
            // sync access handles unlock asynchronously). Anything else is
            // a real recovery bug and must fail the oracle immediately.
            Err(PagedbError::AlreadyLocked) | Err(PagedbError::AlreadyOpen) => {
                sleep_ms(100).await;
            }
            Err(e) => panic!("reopen failed: {e:?}"),
        }
    }
    panic!("could not reopen {root} within retry budget");
}

#[derive(PartialEq)]
enum Expect {
    Old,
    Ambiguous,
    New,
}

async fn run_kill_phase(root: &str, phase: &str, expect: Expect) {
    support::cleanup_dir(root).await;
    driver::oracle_seed(root.to_string()).await.unwrap();

    let (worker, msgs) = spawn_oracle_worker(root, phase);
    // Wait for the phase beacon (or a driver error).
    let mut beacon = None;
    for _ in 0..600 {
        let msg = {
            let mut b = msgs.borrow_mut();
            if b.is_empty() {
                None
            } else {
                Some(b.remove(0))
            }
        };
        if let Some(m) = msg {
            beacon = Some(m);
            break;
        }
        sleep_ms(50).await;
    }
    let beacon = beacon.unwrap_or_else(|| panic!("{phase}: no beacon within 30s"));
    assert!(
        beacon.starts_with("phase-reached:"),
        "{phase}: unexpected driver message: {beacon}"
    );
    // Terminate mid-flight: the park guarantees the worker sits INSIDE the
    // instrumented operation when this lands.
    worker.terminate();

    let db = reopen_with_retry(root).await;
    let r = db.begin_read().await.unwrap();
    // The baseline commit is durable in EVERY phase.
    assert_eq!(
        r.get(b"baseline").await.unwrap().as_deref(),
        Some(b"committed-gen-1".as_ref()),
        "{phase}: baseline lost"
    );
    let victim = r.get(b"victim").await.unwrap();
    drop(r);
    let segment = db.open_segment(REALM, "doomed.seg").await;

    match expect {
        Expect::Old => {
            assert_eq!(victim, None, "{phase}: doomed commit leaked");
            assert!(segment.is_err(), "{phase}: doomed segment visible");
        }
        Expect::New => {
            assert_eq!(
                victim.as_deref(),
                Some(b"uncertain-gen-2".as_ref()),
                "{phase}: published commit lost"
            );
            let reader = segment.unwrap_or_else(|e| panic!("{phase}: segment unreadable: {e:?}"));
            assert!(
                reader
                    .read_page(1)
                    .await
                    .unwrap()
                    .starts_with(b"doomed-segment-page"),
                "{phase}: segment page corrupt"
            );
        }
        Expect::Ambiguous => match victim.as_deref() {
            None => assert!(segment.is_err(), "{phase}: torn state - segment without KV"),
            Some(v) => {
                assert_eq!(v, b"uncertain-gen-2".as_ref(), "{phase}: torn victim value");
                let reader = segment
                    .unwrap_or_else(|e| panic!("{phase}: new-state segment unreadable: {e:?}"));
                assert!(
                    reader
                        .read_page(1)
                        .await
                        .unwrap()
                        .starts_with(b"doomed-segment-page")
                );
            }
        },
    }

    // Post-recovery the database must accept new commits.
    let mut w = db.begin_write().await.unwrap();
    w.put(b"post-recovery", b"ok").await.unwrap();
    w.commit().await.unwrap();
}

// ── The seven kill beacons ────────────────────────────────────────────────────

#[wasm_bindgen_test]
async fn kill_mid_vectored_write_recovers_old() {
    run_kill_phase("orc-1", "mid-vectored-write", Expect::Old).await;
}

#[wasm_bindgen_test]
async fn kill_pages_written_pre_sync_recovers_old() {
    run_kill_phase("orc-2", "pages-written-pre-sync", Expect::Old).await;
}

#[wasm_bindgen_test]
async fn kill_pages_synced_pre_header_write_recovers_old() {
    run_kill_phase("orc-3", "pages-synced-pre-header-write", Expect::Old).await;
}

#[wasm_bindgen_test]
async fn kill_header_written_pre_sync_is_atomic_either_way() {
    run_kill_phase("orc-4", "header-written-pre-sync", Expect::Ambiguous).await;
}

#[wasm_bindgen_test]
async fn kill_header_synced_pre_rename_recovers_new_via_reconcile() {
    run_kill_phase("orc-5", "header-synced-pre-rename", Expect::New).await;
}

#[wasm_bindgen_test]
async fn kill_during_sync_dir_recovers_new_via_reconcile() {
    run_kill_phase("orc-6", "during-sync-dir", Expect::New).await;
}

#[wasm_bindgen_test]
async fn kill_after_sync_dir_pre_gc_recovers_new() {
    run_kill_phase("orc-7", "after-sync-dir-pre-gc", Expect::New).await;
}

// ── FaultVfs error-injection (error-return paths, same worker) ────────────────

/// `published`: whether the injected error lands AFTER the header flip.
/// A post-publication error (e.g. at sync_dir) surfaces to the caller but
/// CANNOT unpublish - reopen serves the new state, with the segment
/// recovered via reconcile. That asymmetry is the commit protocol working
/// as designed, and the oracle pins it.
async fn run_error_injection(root: &str, kind: OpKind, at: u64, published: bool) {
    support::cleanup_dir(root).await;
    driver::oracle_seed(root.to_string()).await.unwrap();

    {
        let action = if published {
            Action::InjectErrorPersistent
        } else {
            Action::InjectError
        };
        let vfs = FaultVfs::new_unarmed(OpfsVfs::with_root(root).await.unwrap(), kind, at, action);
        let db = Db::open_existing(vfs.clone(), KEK, PAGE, REALM)
            .await
            .unwrap();
        // A segment so the commit exercises the promote path (plain KV
        // commits never call sync_dir); sealed UNARMED - seal shares the
        // same VFS ops as the commit protocol.
        let mut sw = db
            .create_segment(REALM, pagedb::SegmentKind::Unspecified)
            .await
            .unwrap();
        sw.append_page(pagedb::SegmentPageKind::Data, b"inject-page")
            .await
            .unwrap();
        let meta = sw.seal().await.unwrap();
        let mut w = db.begin_write().await.unwrap();
        w.put(b"victim", b"should-not-commit").await.unwrap();
        w.link_segment("inject.seg", &meta).await.unwrap();
        vfs.arm();
        let err = w.commit().await.expect_err("injected fault must surface");
        if published {
            assert!(
                matches!(err, PagedbError::DurablyCommittedButUnpublished { .. }),
                "post-publication failure must require reopen, got {err:?}"
            );
        } else {
            assert!(
                matches!(err, PagedbError::Io(_)),
                "pre-publication failure must remain an I/O error, got {err:?}"
            );
        }
        assert!(vfs.fired(), "trigger did not fire");
    }

    // A fresh open serves exactly the correct side of the publication line.
    let db = reopen_with_retry(root).await;
    let r = db.begin_read().await.unwrap();
    assert_eq!(
        r.get(b"baseline").await.unwrap().as_deref(),
        Some(b"committed-gen-1".as_ref())
    );
    let victim = r.get(b"victim").await.unwrap();
    drop(r);
    if published {
        assert_eq!(victim.as_deref(), Some(b"should-not-commit".as_ref()));
        let reader = db
            .open_segment(REALM, "inject.seg")
            .await
            .expect("published segment must be recoverable via reconcile");
        assert!(
            reader
                .read_page(1)
                .await
                .unwrap()
                .starts_with(b"inject-page")
        );
    } else {
        assert_eq!(victim, None);
        assert!(db.open_segment(REALM, "inject.seg").await.is_err());
    }
}

#[wasm_bindgen_test]
async fn inject_error_mid_vectored_write_aborts_cleanly() {
    run_error_injection("orc-e1", OpKind::VectoredSubWrite, 1, false).await;
}

#[wasm_bindgen_test]
async fn inject_error_at_sync_dir_aborts_cleanly() {
    run_error_injection("orc-e2", OpKind::SyncDirBefore, 1, true).await;
}

// ── Manifest/data mismatch ────────────────────────────────────────────────────

/// A committed manifest entry whose physical file has vanished (external
/// interference / partial hardware loss) must surface as a typed error on
/// open - never a trap, never silent fabrication.
#[wasm_bindgen_test]
async fn manifest_referencing_missing_physical_file_fails_typed() {
    use pagedb::vfs::opfs::manifest::Manifest;
    use pagedb::vfs::opfs::registry::FileRegistry;
    use pagedb::vfs::{OpenMode, Vfs};

    let dir = support::test_dir("orc-mismatch").await;
    let reg = FileRegistry::new();
    let phys;
    {
        let m = Manifest::load(&dir, &reg).await.unwrap();
        let id = m.create_file("/vanishing.db").unwrap();
        phys = format!("{id:016x}");
        let f = reg.open(&dir, &phys, true, false).await.unwrap();
        f.write_at(0, b"here today").unwrap();
        f.flush().unwrap();
        m.commit().unwrap();
    }
    // External interference: the physical file disappears.
    let ropts = web_sys::FileSystemRemoveOptions::new();
    ropts.set_recursive(false);
    JsFuture::from(dir.remove_entry_with_options(&phys, &ropts))
        .await
        .unwrap();

    // The namespace still references it; opening must fail typed.
    let vfs = OpfsVfs::with_root("orc-mismatch").await.unwrap();
    let err = vfs
        .open("/vanishing.db", OpenMode::ReadWrite)
        .await
        .err()
        .expect("open of vanished physical file must fail");
    assert!(
        matches!(err, PagedbError::Io(_)),
        "typed error, got {err:?}"
    );
}
