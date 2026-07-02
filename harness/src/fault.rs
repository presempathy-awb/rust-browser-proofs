//! Fault/beacon VFS wrappers for the crash oracle.
//!
//! [`FaultVfs`] wraps any `Vfs` and either injects a typed error or PARKS
//! (awaits a never-resolving future) when a protocol-relevant operation
//! counter reaches its trigger. Parking models a worker about to be
//! terminated mid-operation: inside the sacrificial oracle worker, the
//! wrapper posts a phase beacon and stalls, and the test kills the worker -
//! no destructors, no closes, exactly like a real crash.
//!
//! Counted operations (`OpKind`) map to pagedb's commit protocol:
//! vectored page writes, per-request sub-writes, `sync()` calls, header
//! writes (plain `write_at` on an already-synced file), `rename`, and
//! `sync_dir`. The oracle scripts pick `(OpKind, n)` cut points.

#![cfg(target_arch = "wasm32")]

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use pagedb::Result;
use pagedb::errors::PagedbError;
use pagedb::vfs::traits::{Vfs, VfsFile};
use pagedb::vfs::types::{OpenMode, ReadReq, WriteReq};

/// Operations the oracle can trigger on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// A single request inside `write_at_vectored` (sub-write granularity).
    VectoredSubWrite,
    /// A completed `write_at_vectored` call.
    VectoredWrite,
    /// A plain `write_at` call.
    Write,
    /// A `sync()` call (counted BEFORE executing).
    Sync,
    /// A `rename` call (counted BEFORE executing).
    Rename,
    /// A `sync_dir` call: `Before` parks pre-commit, `After` post-commit.
    SyncDirBefore,
    SyncDirAfter,
}

/// What happens when the trigger fires.
#[derive(Clone, Copy)]
pub enum Action {
    /// Return a typed I/O error (error-path proof).
    InjectError,
    /// Post `phase-reached` to the worker's owner and await forever
    /// (termination-path proof; only meaningful inside the oracle worker).
    Park,
}

struct TriggerState {
    kind: OpKind,
    at: u64,
    action: Action,
    counts: Mutex<std::collections::HashMap<&'static str, u64>>,
    fired: AtomicU64,
    /// Occurrence counting starts only once armed: open/seal/reconcile use
    /// the same VFS ops as the commit protocol, so unarmed phases would
    /// otherwise burn the occurrence budget before the doomed commit runs.
    armed: std::sync::atomic::AtomicBool,
}

/// `Vfs` wrapper with a single (kind, occurrence, action) trigger.
#[derive(Clone)]
pub struct FaultVfs<V> {
    inner: V,
    trig: Arc<TriggerState>,
}

impl<V: Vfs + Clone> FaultVfs<V> {
    pub fn new(inner: V, kind: OpKind, at: u64, action: Action) -> Self {
        Self::build(inner, kind, at, action, true)
    }

    /// Trigger starts DISARMED; call [`FaultVfs::arm`] at the exact
    /// protocol point counting should begin (typically right before the
    /// doomed `commit()`).
    pub fn new_unarmed(inner: V, kind: OpKind, at: u64, action: Action) -> Self {
        Self::build(inner, kind, at, action, false)
    }

    fn build(inner: V, kind: OpKind, at: u64, action: Action, armed: bool) -> Self {
        FaultVfs {
            inner,
            trig: Arc::new(TriggerState {
                kind,
                at,
                action,
                counts: Mutex::new(Default::default()),
                fired: AtomicU64::new(0),
                armed: std::sync::atomic::AtomicBool::new(armed),
            }),
        }
    }

    pub fn arm(&self) {
        self.trig.armed.store(true, Ordering::Relaxed);
    }

    pub fn fired(&self) -> bool {
        self.trig.fired.load(Ordering::Relaxed) > 0
    }
}

fn key(kind: OpKind) -> &'static str {
    match kind {
        OpKind::VectoredSubWrite => "vsub",
        OpKind::VectoredWrite => "vw",
        OpKind::Write => "w",
        OpKind::Sync => "s",
        OpKind::Rename => "r",
        OpKind::SyncDirBefore => "sdb",
        OpKind::SyncDirAfter => "sda",
    }
}

impl TriggerState {
    /// Count an occurrence of `kind`; fire if it matches the trigger.
    async fn tick(&self, kind: OpKind) -> Result<()> {
        if kind != self.kind || !self.armed.load(Ordering::Relaxed) {
            return Ok(());
        }
        let n = {
            let mut c = self.counts.lock().unwrap_or_else(|e| e.into_inner());
            let e = c.entry(key(kind)).or_insert(0);
            *e += 1;
            *e
        };
        if n != self.at {
            return Ok(());
        }
        self.fired.fetch_add(1, Ordering::Relaxed);
        match self.action {
            Action::InjectError => Err(PagedbError::Io(std::io::Error::other(format!(
                "fault injected at {kind:?} #{n}"
            )))),
            Action::Park => {
                post_beacon(&format!("phase-reached:{kind:?}#{n}"));
                park_forever().await
            }
        }
    }
}

fn post_beacon(msg: &str) {
    use wasm_bindgen::JsCast;
    if let Some(scope) = js_sys::global().dyn_ref::<web_sys::DedicatedWorkerGlobalScope>() {
        let _ = scope.post_message(&wasm_bindgen::JsValue::from_str(msg));
    }
}

async fn park_forever() -> Result<()> {
    // A promise that never resolves: the worker sits here until terminated.
    // SendWrapper's Future impl keeps the calling trait futures `Send`
    // (JsFuture itself is !Send) with the usual single-realm runtime guard.
    send_wrapper::SendWrapper::new(async {
        let never = js_sys::Promise::new(&mut |_, _| {});
        let _ = wasm_bindgen_futures::JsFuture::from(never).await;
    })
    .await;
    unreachable!("parked worker was not terminated");
}

pub struct FaultFile<F> {
    inner: F,
    trig: Arc<TriggerState>,
}

impl<V: Vfs + Clone> Vfs for FaultVfs<V>
where
    V::File: Sync,
{
    type File = FaultFile<V::File>;
    type LockHandle = V::LockHandle;

    async fn open(&self, path: &str, mode: OpenMode) -> Result<Self::File> {
        let inner = self.inner.open(path, mode).await?;
        Ok(FaultFile {
            inner,
            trig: Arc::clone(&self.trig),
        })
    }

    async fn remove(&self, path: &str) -> Result<()> {
        self.inner.remove(path).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.trig.tick(OpKind::Rename).await?;
        self.inner.rename(from, to).await
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        self.inner.list_dir(path).await
    }

    async fn mkdir_all(&self, path: &str) -> Result<()> {
        self.inner.mkdir_all(path).await
    }

    async fn sync_dir(&self, path: &str) -> Result<()> {
        self.trig.tick(OpKind::SyncDirBefore).await?;
        self.inner.sync_dir(path).await?;
        self.trig.tick(OpKind::SyncDirAfter).await?;
        Ok(())
    }

    async fn lock_exclusive(&self, path: &str) -> Result<Self::LockHandle> {
        self.inner.lock_exclusive(path).await
    }

    async fn lock_shared(&self, path: &str) -> Result<Self::LockHandle> {
        self.inner.lock_shared(path).await
    }
}

// `&self` methods hold `&FaultFile<F>` across awaits; the trait's Send
// bound therefore needs `F: Sync` (OpfsFile is - SendWrapper fields).
impl<F: VfsFile + Sync> VfsFile for FaultFile<F> {
    async fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.inner.read_at(offset, buf).await
    }

    async fn read_at_vectored(&self, reqs: &mut [ReadReq<'_>]) -> Result<()> {
        self.inner.read_at_vectored(reqs).await
    }

    async fn write_at(&mut self, offset: u64, buf: &[u8]) -> Result<usize> {
        self.trig.tick(OpKind::Write).await?;
        self.inner.write_at(offset, buf).await
    }

    /// Re-implemented per request so the oracle can cut MID-vectored-write.
    async fn write_at_vectored(&mut self, reqs: &[WriteReq<'_>]) -> Result<()> {
        for req in reqs {
            self.trig.tick(OpKind::VectoredSubWrite).await?;
            self.inner.write_at(req.offset, req.buf).await.map(|_| ())?;
        }
        self.trig.tick(OpKind::VectoredWrite).await?;
        Ok(())
    }

    async fn sync(&mut self) -> Result<()> {
        self.trig.tick(OpKind::Sync).await?;
        self.inner.sync().await
    }

    async fn truncate(&mut self, len: u64) -> Result<()> {
        self.inner.truncate(len).await
    }

    async fn len(&self) -> Result<u64> {
        self.inner.len().await
    }

    async fn is_empty(&self) -> Result<bool> {
        self.inner.is_empty().await
    }

    fn supports_direct_io(&self) -> bool {
        self.inner.supports_direct_io()
    }
}
