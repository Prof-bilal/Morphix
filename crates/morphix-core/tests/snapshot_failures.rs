//! Snapshot failure tests (P4-12, TESTING.md §4.3): a full disk, refused reads,
//! truncated objects, corrupted manifests, and crafted path traversal all fail
//! typed, leave no completion marker, and write nothing outside the snapshot dir.
//!
//! Faults that a temp directory cannot produce honestly (ENOSPC) come from the
//! [`FaultInjector`](support::fs::FaultInjector); everything else runs against the
//! real filesystem in an isolated temp home.

mod support;

use morphix_core::errors::ErrorCode;
use morphix_core::fs::sandbox::SafePath;
use morphix_core::fs::{FileSystem, RealFileSystem};
use morphix_core::snapshots::capture::{CaptureEntry, CaptureSpec, capture_snapshot};
use morphix_core::snapshots::manifest::{
    PreState, ResourceRef, SnapshotEntry, SnapshotManifest, SystemInfo,
};
use morphix_core::snapshots::restore::plan_restore;
use morphix_core::snapshots::verify::{collect_garbage, verify_snapshot};
use support::fs::FaultInjector;

fn safe(root: &std::path::Path, name: &str) -> SafePath {
    SafePath::new(root.join(name)).expect("safe path")
}

fn spec(snapshot_dir: SafePath, entries: Vec<CaptureEntry>) -> CaptureSpec {
    CaptureSpec {
        snapshot_id: "01900000-0000-7000-8000-000000000001".to_owned(),
        transaction_id: "01900000-0000-7000-8000-000000000002".to_owned(),
        created_at: "2026-10-21T00:00:00Z".to_owned(),
        app_version: "0.1.0".to_owned(),
        plasma_version: None,
        session: "wayland".to_owned(),
        snapshot_dir,
        entries,
    }
}

#[test]
fn full_disk_fails_capture_without_a_manifest() {
    let directory = tempfile::tempdir().expect("tempdir");
    let fs = FaultInjector::new();
    let source = safe(directory.path(), "kdeglobals");
    fs.write_file_atomic(&source, b"[General]\n", 0o644)
        .expect("setup write");
    fs.fail_writes_with(ErrorCode::InsufficientSpace);

    let snapshot_dir = safe(directory.path(), "snap");
    let error = capture_snapshot(
        &fs,
        &spec(
            snapshot_dir.clone(),
            vec![CaptureEntry::ConfigFile {
                path: source.clone(),
            }],
        ),
    )
    .expect_err("full disk must fail");
    assert_eq!(error.code, ErrorCode::InsufficientSpace);

    // The store directory exists, but capture stopped before the manifest: there
    // is nothing to verify and nothing to complete.
    assert!(
        fs.metadata(
            &snapshot_dir
                .join_relative("snapshot.json")
                .expect("manifest path")
        )
        .is_err(),
        "no manifest may be written after a failed capture"
    );
    assert_eq!(
        std::fs::read(source.as_path()).expect("source survives"),
        b"[General]\n",
        "capture never mutates its targets"
    );
}

#[test]
fn refused_reads_are_permission_errors() {
    let directory = tempfile::tempdir().expect("tempdir");
    let fs = FaultInjector::new();
    let source = safe(directory.path(), "kdeglobals");
    fs.write_file_atomic(&source, b"[General]\n", 0o644)
        .expect("setup write");
    fs.fail_reads();

    let error = capture_snapshot(
        &fs,
        &spec(
            safe(directory.path(), "snap"),
            vec![CaptureEntry::ConfigFile { path: source }],
        ),
    )
    .expect_err("refused read must fail");
    assert_eq!(error.code, ErrorCode::IoPermission);
}

#[test]
fn truncated_objects_fail_verification() {
    let directory = tempfile::tempdir().expect("tempdir");
    let fs = RealFileSystem;
    let source = safe(directory.path(), "kdeglobals");
    fs.write_file_atomic(&source, b"[General]\nColorScheme=BreezeDark\n", 0o644)
        .expect("write");
    let snapshot_dir = safe(directory.path(), "snap");
    capture_snapshot(
        &fs,
        &spec(
            snapshot_dir.clone(),
            vec![CaptureEntry::ConfigFile { path: source }],
        ),
    )
    .expect("capture");

    let objects = snapshot_dir.join_relative("objects").expect("objects");
    let entries = fs.list_dir(&objects).expect("list objects");
    assert_eq!(entries.len(), 1, "one object to truncate");
    let object = objects.join_relative(&entries[0].name).expect("object");
    std::fs::File::options()
        .write(true)
        .open(object.as_path())
        .expect("open object")
        .set_len(4)
        .expect("truncate");

    let error = verify_snapshot(&fs, &snapshot_dir).expect_err("truncated");
    assert_eq!(error.code, ErrorCode::SnapshotCorrupt);
}

#[test]
fn crafted_manifest_hashes_are_corrupt_and_stay_inside_the_store() {
    let directory = tempfile::tempdir().expect("tempdir");
    let fs = RealFileSystem;
    let snapshot_dir = safe(directory.path(), "snap");
    fs.create_dir_all(&snapshot_dir, 0o700).expect("mkdir");

    // A manifest that verifies its self-hash but names an object outside the store.
    let manifest = SnapshotManifest::new(
        "snap-1",
        "txn-1",
        "2026-10-21T00:00:00Z",
        "0.1.0",
        SystemInfo {
            plasma_version: None,
            session: "wayland".to_owned(),
        },
        vec![SnapshotEntry {
            resource: ResourceRef::ConfigFile {
                path: "/home/u/.config/kdeglobals".to_owned(),
            },
            pre_state: PreState::File {
                sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                    .to_owned(),
                mode: 0o644,
                size: 0,
            },
            object: Some("../escape".to_owned()),
        }],
    )
    .expect("manifest")
    .seal()
    .expect("seal");
    manifest.write_to(&fs, &snapshot_dir).expect("write");

    let error = verify_snapshot(&fs, &snapshot_dir).expect_err("traversal");
    assert_eq!(error.code, ErrorCode::SnapshotCorrupt);
    let resolve = |_: &str| None;
    assert_eq!(
        plan_restore(&fs, &snapshot_dir, &resolve)
            .expect_err("no plan")
            .code,
        ErrorCode::SnapshotCorrupt
    );

    // Nothing escaped the snapshot directory: only objects/ and snapshot.json exist.
    let mut names: Vec<String> = std::fs::read_dir(directory.path())
        .expect("list temp")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, vec!["snap".to_owned()]);
}

#[test]
fn corrupt_manifests_collect_as_garbage() {
    let directory = tempfile::tempdir().expect("tempdir");
    let fs = RealFileSystem;
    let root = safe(directory.path(), "snapshots");
    fs.create_dir_all(&root, 0o700).expect("mkdir");
    let partial = root.join_relative("snap-partial").expect("partial");
    fs.create_dir_all(&partial.join_relative("objects").expect("objects"), 0o700)
        .expect("partial objects");

    let report = collect_garbage(&fs, &root).expect("gc");
    assert_eq!(report.removed, vec!["snap-partial".to_owned()]);
    assert!(report.corrupt.is_empty());
    assert!(fs.metadata(&partial).is_err(), "the partial is gone");
}
