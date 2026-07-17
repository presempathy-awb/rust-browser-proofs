//! Windows-only cross-process coverage for PageDB's native Tokio VFS locks.
//!
//! The in-process lock table cannot prove `LockFileEx` behavior, so each
//! assertion starts a second copy of this test binary and contends on the same
//! lock file through PageDB's public VFS API.

#![cfg(windows)]

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use pagedb::{
    errors::PagedbError,
    vfs::{Vfs, tokio_backend::TokioVfs},
};

const HELPER_TEST: &str = "lock_holder";
const READY_TIMEOUT: Duration = Duration::from_secs(10);

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "rust-browser-proofs-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("test directory should be creatable");
        Self { path }
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn lock_holder() {
    let Ok(root) = std::env::var("PAGEDB_WINDOWS_LOCK_ROOT") else {
        return;
    };
    let lock_path = std::env::var("PAGEDB_WINDOWS_LOCK_PATH")
        .expect("PAGEDB_WINDOWS_LOCK_PATH must be set for the helper");
    let lock_kind = std::env::var("PAGEDB_WINDOWS_LOCK_KIND")
        .expect("PAGEDB_WINDOWS_LOCK_KIND must be set for the helper");
    let ready = PathBuf::from(
        std::env::var("PAGEDB_WINDOWS_LOCK_READY")
            .expect("PAGEDB_WINDOWS_LOCK_READY must be set for the helper"),
    );
    let release = PathBuf::from(
        std::env::var("PAGEDB_WINDOWS_LOCK_RELEASE")
            .expect("PAGEDB_WINDOWS_LOCK_RELEASE must be set for the helper"),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime should build");
    let vfs = TokioVfs::new(root);

    match lock_kind.as_str() {
        "exclusive" => {
            let _lock = runtime
                .block_on(vfs.lock_exclusive(&lock_path))
                .expect("child should acquire its exclusive lock");
            std::fs::write(&ready, b"ready").expect("child should signal readiness");
            wait_for_file(&release, READY_TIMEOUT, "parent release marker");
        }
        "shared" => {
            let _lock = runtime
                .block_on(vfs.lock_shared(&lock_path))
                .expect("child should acquire its shared lock");
            std::fs::write(&ready, b"ready").expect("child should signal readiness");
            wait_for_file(&release, READY_TIMEOUT, "parent release marker");
        }
        _ => panic!("unsupported PAGEDB_WINDOWS_LOCK_KIND: {lock_kind}"),
    }
}

#[test]
fn exclusive_lock_conflicts_across_windows_processes() {
    assert_conflict("exclusive", ".writer.lock");
}

#[test]
fn shared_lock_conflicts_with_exclusive_request_across_windows_processes() {
    assert_conflict("shared", ".frozen_readers.lock");
}

fn assert_conflict(lock_kind: &str, lock_path: &str) {
    let root = TemporaryDirectory::new(lock_kind);
    let ready = root.path.join("holder.ready");
    let release = root.path.join("holder.release");
    let mut child = Command::new(std::env::current_exe().expect("test binary path"))
        .args(["--exact", HELPER_TEST, "--nocapture"])
        .env("PAGEDB_WINDOWS_LOCK_ROOT", &root.path)
        .env("PAGEDB_WINDOWS_LOCK_PATH", lock_path)
        .env("PAGEDB_WINDOWS_LOCK_KIND", lock_kind)
        .env("PAGEDB_WINDOWS_LOCK_READY", &ready)
        .env("PAGEDB_WINDOWS_LOCK_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("lock-holder child process should start");

    wait_for_child_ready(&mut child, &ready);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime should build");
    let vfs = TokioVfs::new(&root.path);
    let result = runtime.block_on(vfs.lock_exclusive(lock_path));

    std::fs::write(&release, b"release").expect("parent should release child");
    let output = child
        .wait_with_output()
        .expect("lock-holder child should exit");
    assert!(
        output.status.success(),
        "lock-holder child failed (status={}): stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        matches!(result, Err(PagedbError::AlreadyLocked)),
        "exclusive lock should fail while another Windows process holds a {lock_kind} lock"
    );
}

fn wait_for_child_ready(child: &mut Child, ready: &Path) {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if ready.exists() {
            return;
        }
        if let Some(status) = child
            .try_wait()
            .expect("should be able to inspect the child process")
        {
            panic!("lock-holder child exited before signaling readiness: {status}");
        }
        if Instant::now() >= deadline {
            child
                .kill()
                .expect("should be able to terminate a timed-out child");
            let status = child
                .wait()
                .expect("should be able to wait for a timed-out child");
            panic!("lock-holder child did not become ready within {READY_TIMEOUT:?}: {status}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_file(path: &Path, timeout: Duration, description: &str) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description} within {timeout:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
