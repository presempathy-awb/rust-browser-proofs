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
    // File children only (MemVfs reference semantic): "sub" is a Dir.
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
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
        let f = reg
            .open(&dir, "00000000deadbeef", true, false)
            .await
            .unwrap();
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

#[wasm_bindgen_test]
async fn both_slots_corrupt_refuses_load_and_preserves_files() {
    let dir = test_dir("man-both-corrupt").await;
    let reg = FileRegistry::new();

    let keep_phys;
    {
        let m = Manifest::load(&dir, &reg).await.unwrap();
        let id = m.create_file("/keep.db").unwrap();
        keep_phys = format!("{id:016x}");
        m.commit().unwrap();
        // create_file only allocates the ID - the caller creates the
        // physical file (as OpfsVfs::open does).
        let f = reg.open(&dir, &keep_phys, true, false).await.unwrap();
        f.write_at(0, b"precious").unwrap();
        f.flush().unwrap();
    }

    // Corrupt BOTH slots: this is no longer distinguishable-as-fresh.
    for slot in [Manifest::SLOT_A, Manifest::SLOT_B] {
        let f = reg.open(&dir, slot, true, false).await.unwrap();
        f.truncate(0).unwrap();
        f.write_at(0, b"total-garbage-in-this-slot").unwrap();
        f.flush().unwrap();
    }

    // Load must fail typed - NOT come up empty and GC the data files.
    let err = Manifest::load(&dir, &reg)
        .await
        .err()
        .expect("load must refuse");
    assert!(matches!(err, pagedb::errors::PagedbError::Io(_)));

    // The data file survived the refused load.
    let f = reg.open(&dir, &keep_phys, false, false).await.unwrap();
    let mut buf = [0u8; 8];
    assert_eq!(f.read_at(0, &mut buf).unwrap(), 8);
    assert_eq!(&buf, b"precious");
}

#[wasm_bindgen_test]
async fn next_id_never_reuses_a_gc_surviving_orphan() {
    let dir = test_dir("man-id-reuse").await;
    let reg = FileRegistry::new();

    let removed_id;
    {
        let m = Manifest::load(&dir, &reg).await.unwrap();
        m.create_file("/a.db").unwrap();
        removed_id = m.create_file("/b.db").unwrap();
        // Materialise the physical file (create_file only allocates the ID).
        let f = reg
            .open(&dir, &format!("{removed_id:016x}"), true, false)
            .await
            .unwrap();
        f.write_at(0, b"orphan-to-be").unwrap();
        f.flush().unwrap();
        m.commit().unwrap();
        m.remove("/b.db").unwrap();
        m.commit().unwrap();
    }

    // Keep the orphaned physical file OPEN so load-time GC cannot delete it
    // (registry-live skip + locked handle both protect it).
    let held = reg
        .open(&dir, &format!("{removed_id:016x}"), false, false)
        .await
        .unwrap();

    let m2 = Manifest::load(&dir, &reg).await.unwrap();
    let new_id = m2.create_file("/c.db").unwrap();
    assert_ne!(
        new_id, removed_id,
        "reallocating a surviving orphan's ID would collide with its file"
    );
    drop(held);
}

#[wasm_bindgen_test]
async fn remove_of_nonempty_dir_is_rejected() {
    let dir = test_dir("man-rmdir").await;
    let reg = FileRegistry::new();

    let m = Manifest::load(&dir, &reg).await.unwrap();
    m.create_file("/d/inner.db").unwrap();
    let err = m.remove("/d").expect_err("non-empty dir remove must fail");
    assert!(matches!(err, pagedb::errors::PagedbError::Io(_)));
    // File removes and empty-dir removes still work.
    m.remove("/d/inner.db").unwrap();
    m.remove("/d").unwrap();
}

#[wasm_bindgen_test]
async fn namespace_invariants_are_enforced() {
    let dir = test_dir("man-invariants").await;
    let reg = FileRegistry::new();
    let m = Manifest::load(&dir, &reg).await.unwrap();

    m.create_file("/d/file.db").unwrap();
    m.mkdir_all("/d/subdir").unwrap();

    // Directory rename is rejected (would strand descendants).
    assert!(m.rename("/d", "/e").is_err());
    // Renaming onto a directory destination is rejected.
    assert!(m.rename("/d/file.db", "/d/subdir").is_err());
    // A file cannot serve as a parent directory.
    assert!(m.create_file("/d/file.db/child").is_err());
    assert!(m.mkdir_all("/d/file.db/sub").is_err());
    // mkdir_all over an existing file path is rejected.
    assert!(m.mkdir_all("/d/file.db").is_err());
}

/// The two on-disk states a crash INSIDE Manifest::commit can leave are
/// both pinned: a torn slot (covered by the torn-slot tests above) and a
/// fully-written valid slot whose in-memory adoption was lost with the
/// worker. This test pins the second: the slot WRITE is the publication
/// point - a next-seq record present on disk is adopted by recovery even
/// though the committing instance never flipped its in-memory state.
#[wasm_bindgen_test]
async fn crash_after_slot_write_adopts_new_namespace() {
    use pagedb::vfs::opfs::manifest::{ManifestRecord, encode_manifest};

    let dir = test_dir("man-slot-publish").await;
    let reg = FileRegistry::new();

    let m = Manifest::load(&dir, &reg).await.unwrap();
    m.create_file("/old.db").unwrap();
    m.commit().unwrap(); // seq 1
    let inactive = if m.active_slot_name() == Manifest::SLOT_A {
        Manifest::SLOT_B
    } else {
        Manifest::SLOT_A
    };
    drop(m);

    // Simulate: next commit wrote its full record to the inactive slot,
    // then the worker died before doing anything else.
    let rec = ManifestRecord {
        seq: 2,
        entries: vec![
            ("/new.db".to_string(), EntryKind::File(42)),
            ("/old.db".to_string(), EntryKind::File(1)),
        ],
    };
    let bytes = encode_manifest(&rec).unwrap();
    let f = reg.open(&dir, inactive, true, false).await.unwrap();
    f.truncate(0).unwrap();
    f.write_at(0, &bytes).unwrap();
    f.flush().unwrap();
    drop(f);

    let m2 = Manifest::load(&dir, &reg).await.unwrap();
    assert_eq!(m2.resolve("/new.db"), Some(EntryKind::File(42)));
    assert!(m2.resolve("/old.db").is_some());
}

/// GC ownership: entries this backend did not coin ({id:016x} names) are
/// NEVER garbage-collected - the directory may be shared (or be the OPFS
/// root), and foreign files must survive every load.
#[wasm_bindgen_test]
async fn gc_never_touches_unrecognized_entries() {
    let dir = test_dir("man-foreign").await;
    let reg = FileRegistry::new();

    // A foreign file some other software placed in the same directory.
    let f = reg.open(&dir, "user-notes.txt", true, false).await.unwrap();
    f.write_at(0, b"not ours").unwrap();
    f.flush().unwrap();
    drop(f);

    // Several loads (each runs GC) with commits in between.
    for i in 0..2 {
        let m = Manifest::load(&dir, &reg).await.unwrap();
        m.create_file(&format!("/mine-{i}.db")).unwrap();
        m.commit().unwrap();
    }

    let f = reg
        .open(&dir, "user-notes.txt", false, false)
        .await
        .unwrap();
    let mut buf = [0u8; 8];
    assert_eq!(f.read_at(0, &mut buf).unwrap(), 8);
    assert_eq!(&buf, b"not ours");
}
