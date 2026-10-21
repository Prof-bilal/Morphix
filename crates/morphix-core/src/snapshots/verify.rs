//! Snapshot verification and startup garbage collection (P4-09).
//!
//! **Ownership.** The only decider of whether a snapshot on disk is complete. Apply
//! and restore paths call [`verify_snapshot`] before touching user state; a snapshot
//! that does not verify is never the basis of a mutation (TESTING.md §4.3).
//!
//! **Startup GC (TRANSACTIONS.md §6).** Snapshots orphaned by a failed pre-mutation
//! abort — no verifiable manifest — are removed on startup. Snapshots with a valid
//! manifest but damaged objects are *reported, not deleted*: silent deletion would
//! destroy the evidence a recovery report needs.

use crate::errors::{ErrorCode, MorphixError, Result};
use crate::fs::sandbox::SafePath;
use crate::fs::tree::EntryKind;
use crate::fs::{FileSystem, tree};

use super::manifest::SnapshotManifest;
use super::store::ObjectStore;
use crate::storage::db::Storage;

/// What a successful verification established.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    /// The verified manifest.
    pub manifest: SnapshotManifest,
    /// Number of objects read back and hash-checked.
    pub objects: usize,
    /// Total object bytes read back.
    pub bytes: u64,
}

/// Verifies a snapshot directory from the ground up: manifest parses and its
/// self-hash matches, every referenced object is present, and every object's
/// content matches its hash. File entries are additionally cross-checked against
/// their recorded size.
///
/// Nothing here consults the database or the live system: this proves the *store*,
/// not that the world still matches the pre-state.
///
/// # Errors
///
/// Returns `E_SNAPSHOT_UNREADABLE` for a missing manifest or object, and
/// `E_SNAPSHOT_CORRUPT` for a manifest or object that fails its hash.
pub fn verify_snapshot<F: FileSystem>(
    fs: &F,
    snapshot_dir: &SafePath,
) -> Result<VerificationReport> {
    let manifest = SnapshotManifest::read_from(fs, snapshot_dir)?;
    let store = ObjectStore::open(fs, snapshot_dir)?;
    let mut objects = 0;
    let mut bytes = 0;
    for entry in &manifest.entries {
        let Some(object) = &entry.object else {
            continue;
        };
        let hash = super::store::ObjectHash::parse(object)?;
        let content = store.get(&hash)?;
        bytes += content.len() as u64;
        objects += 1;
        if let super::manifest::PreState::File { size, .. } = &entry.pre_state
            && content.len() as u64 != *size
        {
            return Err(MorphixError::new(
                ErrorCode::SnapshotCorrupt,
                "a snapshot object does not match its recorded size",
            )
            .with_context("snapshot_id", &manifest.snapshot_id));
        }
    }
    Ok(VerificationReport {
        manifest,
        objects,
        bytes,
    })
}

/// Verifies a snapshot and only then records its completion in the database.
///
/// This is the seam TRANSACTIONS.md §6 pins down: the `snapshot_complete` marker
/// (the `complete` row state) is written if and only if every object verified.
/// Verification failure leaves the row `partial`, so the startup collector treats
/// the snapshot as an orphaned abort, never as a basis for mutation.
///
/// # Errors
///
/// Returns the verification error when the snapshot does not verify, and
/// `E_INTERNAL` when the row is missing or already moved past `partial`.
pub fn complete_snapshot<F: FileSystem>(
    fs: &F,
    storage: &mut Storage,
    snapshot_id: &str,
    snapshot_dir: &SafePath,
) -> Result<VerificationReport> {
    let report = verify_snapshot(fs, snapshot_dir)?;
    let manifest = &report.manifest;
    // Object stores are capped per file, so saturation is unreachable in practice;
    // the database column is a signed integer.
    let total_bytes = i64::try_from(report.bytes).unwrap_or(i64::MAX);
    let marked = storage.mark_snapshot_complete(
        snapshot_id,
        &manifest.manifest_hash,
        manifest.entries.len() as i64,
        total_bytes,
    )?;
    if !marked {
        return Err(MorphixError::new(
            ErrorCode::Internal,
            "the snapshot row is not partial; completion refused",
        )
        .with_context("snapshot_id", snapshot_id));
    }
    Ok(report)
}

/// What startup garbage collection did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcReport {
    /// Snapshot directory names that were partial and have been removed.
    pub removed: Vec<String>,
    /// Snapshot ids with a valid manifest but damaged objects. Left in place for
    /// recovery reporting; never silently deleted.
    pub corrupt: Vec<String>,
}

/// Removes orphaned partial snapshots under `snapshots_root`.
///
/// A child is partial when it has no verifiable manifest: no manifest at all, an
/// unparsable one, or a hash mismatch. Symlinked children are skipped and reported
/// as corrupt rather than followed or unlinked. Manifest-valid children with
/// damaged objects are reported, not removed.
///
/// # Errors
///
/// Returns a typed I/O error when the root cannot be listed. Individual removal
/// failures are reported as `E_IO`, not swallowed: a half-removed partial must be
/// visible, not silently kept.
pub fn collect_garbage<F: FileSystem>(fs: &F, snapshots_root: &SafePath) -> Result<GcReport> {
    let mut report = GcReport::default();
    let children = match fs.list_dir(snapshots_root) {
        Err(error) if error.code == ErrorCode::Io => return Ok(report),
        Err(error) => return Err(error),
        Ok(children) => children,
    };
    for child in children {
        if child.kind == EntryKind::Symlink {
            report.corrupt.push(child.name.clone());
            continue;
        }
        if child.kind != EntryKind::Directory {
            continue;
        }
        let directory = snapshots_root.join_relative(&child.name).map_err(|error| {
            MorphixError::new(ErrorCode::Internal, "a snapshot path is not safe")
                .with_context("reason", error.to_string())
        })?;
        match SnapshotManifest::read_from(fs, &directory) {
            Err(_) => {
                tree::remove_tree(directory.as_path())?;
                report.removed.push(child.name.clone());
            }
            Ok(manifest) => {
                if verify_snapshot(fs, &directory).is_err() {
                    report.corrupt.push(manifest.snapshot_id.clone());
                }
            }
        }
    }
    report.removed.sort();
    report.corrupt.sort();
    Ok(report)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{collect_garbage, verify_snapshot};
    use crate::fs::sandbox::SafePath;
    use crate::fs::{FileSystem, RealFileSystem};
    use crate::snapshots::capture::{CaptureEntry, CaptureSpec, capture_snapshot};

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

    fn safe(root: &std::path::Path, name: &str) -> SafePath {
        SafePath::new(root.join(name)).expect("safe path")
    }

    #[test]
    fn verification_counts_objects_and_bytes() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let source = safe(directory.path(), "kdeglobals");
        fs.write_file_atomic(&source, b"[General]\n", 0o644)
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

        let report = verify_snapshot(&fs, &snapshot_dir).expect("verify");
        assert_eq!(report.objects, 1);
        assert_eq!(report.bytes, 10);
        assert_eq!(report.manifest.entries.len(), 1);
    }

    #[test]
    fn missing_objects_are_unreadable_and_tampered_ones_corrupt() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let source = safe(directory.path(), "kdeglobals");
        fs.write_file_atomic(&source, b"data", 0o644)
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
        for entry in fs.list_dir(&objects).expect("list objects") {
            let path = objects.join_relative(&entry.name).expect("object path");
            fs.write_file_atomic(&path, b"tampered-content!!", 0o600)
                .expect("tamper");
        }
        let error = verify_snapshot(&fs, &snapshot_dir).expect_err("tampered");
        assert_eq!(error.code, crate::errors::ErrorCode::SnapshotCorrupt);
    }

    #[test]
    fn garbage_collection_removes_partials_and_reports_corrupt() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let root = safe(directory.path(), "snapshots");
        fs.create_dir_all(&root, 0o700).expect("mkdir");

        // A complete snapshot: kept.
        let source = safe(directory.path(), "kdeglobals");
        fs.write_file_atomic(&source, b"[General]\n", 0o644)
            .expect("write");
        let complete = root.join_relative("snap-complete").expect("complete");
        capture_snapshot(
            &fs,
            &spec(complete, vec![CaptureEntry::ConfigFile { path: source }]),
        )
        .expect("capture");

        // A partial: manifest missing. Removed.
        let partial = root.join_relative("snap-partial").expect("partial");
        fs.create_dir_all(&partial.join_relative("objects").expect("objects"), 0o700)
            .expect("partial objects");

        // A corrupt manifest. Removed.
        let corrupt_manifest = root.join_relative("snap-bad").expect("bad");
        fs.create_dir_all(&corrupt_manifest, 0o700)
            .expect("bad dir");
        let manifest_path = corrupt_manifest
            .join_relative("snapshot.json")
            .expect("path");
        fs.write_file_atomic(&manifest_path, b"{broken", 0o600)
            .expect("bad manifest");

        // A valid manifest with a missing object: reported, kept.
        let damaged = root.join_relative("snap-damaged").expect("damaged");
        let source_two = safe(directory.path(), "other");
        fs.write_file_atomic(&source_two, b"other", 0o644)
            .expect("write two");
        capture_snapshot(
            &fs,
            &spec(
                damaged.clone(),
                vec![CaptureEntry::ConfigFile { path: source_two }],
            ),
        )
        .expect("capture two");
        let objects = damaged.join_relative("objects").expect("objects");
        for entry in fs.list_dir(&objects).expect("list") {
            std::fs::remove_file(objects.join_relative(&entry.name).expect("p").as_path())
                .expect("delete object");
        }

        let report = collect_garbage(&fs, &root).expect("gc");
        assert_eq!(report.removed, vec!["snap-bad", "snap-partial"]);
        assert_eq!(report.corrupt.len(), 1);
        assert!(root.join_relative("snap-complete").is_ok());
        assert!(fs.metadata(&damaged).is_ok(), "damaged snapshots are kept");
        assert!(
            fs.metadata(&root.join_relative("snap-complete").expect("kept"))
                .is_ok()
        );
    }

    #[test]
    fn completion_is_never_recorded_without_verification() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let mut storage = crate::storage::db::Storage::open_in_memory().expect("database");
        storage
            .connection()
            .execute(
                "INSERT INTO transactions (transaction_id, kind, state, app_version, started_at) \
                 VALUES ('txn-1', 'apply_pack', 'snapshotting', '0.1.0', '2026-10-21T00:00:00Z')",
                [],
            )
            .expect("stub transaction");

        let source = safe(directory.path(), "kdeglobals");
        fs.write_file_atomic(&source, b"data", 0o644)
            .expect("write");
        let snapshot_dir = safe(directory.path(), "snap");
        let manifest = capture_snapshot(
            &fs,
            &spec(
                snapshot_dir.clone(),
                vec![CaptureEntry::ConfigFile { path: source }],
            ),
        )
        .expect("capture");
        storage
            .save_snapshot(
                &crate::storage::repo::snapshots::StoredSnapshot {
                    snapshot_id: manifest.snapshot_id.clone(),
                    transaction_id: "txn-1".to_owned(),
                    kind: "apply".to_owned(),
                    state: "partial".to_owned(),
                    manifest_path: snapshot_dir
                        .join_relative("snapshot.json")
                        .expect("manifest path")
                        .as_path()
                        .display()
                        .to_string(),
                    manifest_hash: String::new(),
                    entry_count: 0,
                    total_bytes: 0,
                    app_version: "0.1.0".to_owned(),
                    created_at: "2026-10-21T00:00:00Z".to_owned(),
                },
                &[],
            )
            .expect("save partial");

        // Damage the object behind the store's back.
        let objects = snapshot_dir.join_relative("objects").expect("objects");
        for entry in fs.list_dir(&objects).expect("list") {
            let path = objects.join_relative(&entry.name).expect("object");
            fs.write_file_atomic(&path, b"tampered!!", 0o600)
                .expect("tamper");
        }

        let error =
            super::complete_snapshot(&fs, &mut storage, &manifest.snapshot_id, &snapshot_dir)
                .expect_err("damaged snapshot must not complete");
        assert_eq!(error.code, crate::errors::ErrorCode::SnapshotCorrupt);
        let (row, _) = storage
            .snapshot_detail(&manifest.snapshot_id)
            .expect("detail");
        assert_eq!(
            row.state, "partial",
            "the completion marker was not written"
        );
        assert!(row.manifest_hash.is_empty());
    }

    #[test]
    fn garbage_collection_on_a_missing_root_is_a_no_op() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let report = collect_garbage(&fs, &safe(directory.path(), "nope")).expect("gc");
        assert_eq!(report, super::GcReport::default());
    }
}
