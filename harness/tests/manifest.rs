//! Task 3 RED->GREEN: logical-path manifest with A/B crash protocol.
//!
//! Observable contract:
//! - Namespace mutations become durable ONLY at `commit()` (the `sync_dir`
//!   durability point); reload after commit sees exactly the committed map.
//! - A corrupted (torn) slot never breaks recovery: the other slot's
//!   highest-valid-seq record wins.
//! - Physical files not referenced by the winning manifest are garbage
//!   collected on load.

#![cfg(target_arch = "wasm32")]

mod support;

use pagedb::vfs::opfs::manifest::{EntryKind, Manifest};
use pagedb::vfs::opfs::registry::FileRegistry;
use support::test_dir;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

#[wasm_bindgen_test]
async fn commit_then_reload_sees_committed_namespace() {
    let dir = test_dir("man-reload").await;
    let reg = FileRegistry::new();

    let phys_id;
    {
        let m = Manifest::load(&dir, &reg).await.unwrap();
        m.mkdir_all("/seg/.staging").unwrap();
        phys_id = m.create_file("/main.db").unwrap();
        m.commit().unwrap();
    }

    let m2 = Manifest::load(&dir, &reg).await.unwrap();
    assert_eq!(m2.resolve("/main.db"), Some(EntryKind::File(phys_id)));
    assert_eq!(m2.resolve("/seg/.staging"), Some(EntryKind::Dir));
    assert_eq!(m2.resolve("/nope"), None);
}

#[wasm_bindgen_test]
async fn uncommitted_mutations_do_not_survive_reload() {
    let dir = test_dir("man-uncommitted").await;
    let reg = FileRegistry::new();

    {
        let m = Manifest::load(&dir, &reg).await.unwrap();
        m.create_file("/committed.db").unwrap();
        m.commit().unwrap();
        m.create_file("/uncommitted.db").unwrap();
        // dropped without commit
    }

    let m2 = Manifest::load(&dir, &reg).await.unwrap();
    assert!(m2.resolve("/committed.db").is_some());
    assert_eq!(m2.resolve("/uncommitted.db"), None);
}

#[wasm_bindgen_test]
async fn rename_while_referenced_updates_mapping_and_overwrites() {
    let dir = test_dir("man-rename").await;
    let reg = FileRegistry::new();

    let m = Manifest::load(&dir, &reg).await.unwrap();
    let from_id = m.create_file("/seg/.staging/aa").unwrap();
    let _clobbered = m.create_file("/seg/aa").unwrap();
    m.commit().unwrap();

    // POSIX overwrite semantics: destination entry replaced.
    m.rename("/seg/.staging/aa", "/seg/aa").unwrap();
    m.commit().unwrap();

    assert_eq!(m.resolve("/seg/aa"), Some(EntryKind::File(from_id)));
    assert_eq!(m.resolve("/seg/.staging/aa"), None);

    let m2 = Manifest::load(&dir, &reg).await.unwrap();
    assert_eq!(m2.resolve("/seg/aa"), Some(EntryKind::File(from_id)));
}

#[wasm_bindgen_test]
async fn list_dir_returns_direct_children_only() {
    let dir = test_dir("man-listdir").await;
    let reg = FileRegistry::new();

    let m = Manifest::load(&dir, &reg).await.unwrap();
    m.create_file("/d/a").unwrap();
    m.create_file("/d/b").unwrap();
    m.mkdir_all("/d/sub").unwrap();
    m.create_file("/d/sub/deep").unwrap();

    let mut names = m.list_dir("/d").unwrap();
    names.sort();
    assert_eq!(names, vec!["a".to_string(), "b".to_string(), "sub".to_string()]);
}

#[wasm_bindgen_test]
async fn torn_inactive_slot_never_affects_recovery() {
    let dir = test_dir("man-torn-inactive").await;
    let reg = FileRegistry::new();

    {
        let m = Manifest::load(&dir, &reg).await.unwrap();
        m.create_file("/keep.db").unwrap();
        m.commit().unwrap();
    }

    // Corrupt the INACTIVE slot (the one the NEXT commit would target).
    // Recovery must keep serving the committed state.
    for slot in [Manifest::SLOT_A, Manifest::SLOT_B] {
        let f = reg.open(&dir, slot, true, false).await.unwrap();
        if f.size().unwrap() == 0 {
            f.write_at(0, b"garbage-torn-write").unwrap();
            f.flush().unwrap();
        }
    }

    let m2 = Manifest::load(&dir, &reg).await.unwrap();
    assert!(m2.resolve("/keep.db").is_some());
}

#[wasm_bindgen_test]
async fn corrupted_active_slot_falls_back_to_previous_commit() {
    let dir = test_dir("man-torn-active").await;
    let reg = FileRegistry::new();

    {
        let m = Manifest::load(&dir, &reg).await.unwrap();
        m.create_file("/gen1.db").unwrap();
        m.commit().unwrap(); // seq 1
        m.create_file("/gen2.db").unwrap();
        m.commit().unwrap(); // seq 2
    }

    // Corrupt the ACTIVE slot (highest seq). Recovery must fall back to the
    // seq-1 record: gen1 present, gen2 lost (pre-durability by definition).
    {
        let m = Manifest::load(&dir, &reg).await.unwrap();
        let active = m.active_slot_name();
        let f = reg.open(&dir, active, false, false).await.unwrap();
        f.write_at(8, b"XXXXXXXX").unwrap(); // stomp the record mid-header
        f.flush().unwrap();
    }

    let m2 = Manifest::load(&dir, &reg).await.unwrap();
    assert!(m2.resolve("/gen1.db").is_some());
    assert_eq!(m2.resolve("/gen2.db"), None);
}

#[wasm_bindgen_test]
async fn orphaned_physical_file_removed_on_load() {
    let dir = test_dir("man-gc").await;
    let reg = FileRegistry::new();

    {
        let m = Manifest::load(&dir, &reg).await.unwrap();
        m.create_file("/real.db").unwrap();
        m.commit().unwrap();
    }

    // A physical file never committed to the manifest: an orphan.
    {
        let f = reg.open(&dir, "00000000deadbeef", true, false).await.unwrap();
        f.write_at(0, b"orphan").unwrap();
        f.flush().unwrap();
    }

    let _m2 = Manifest::load(&dir, &reg).await.unwrap();

    // The orphan must be gone: opening without create fails NotFound.
    let err = reg
        .open(&dir, "00000000deadbeef", false, false)
        .await
        .err()
        .expect("orphan should have been GC'd");
    assert!(matches!(err, pagedb::errors::PagedbError::Io(_)));
}
