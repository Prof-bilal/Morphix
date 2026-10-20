//! The local pack store: no-follow copy into a stable per-pack directory (P2-11).
//!
//! Installation copies a validated directory into `<data>/packs/<pack-id>/` through a
//! staging directory and a rename. The post-copy tree hash must equal the hash that was
//! validated, otherwise the install is refused and the staging tree is removed. Nothing is
//! registered in the database here; the service does that only after this function
//! succeeds (SECURITY.md §9, AC-18).

use std::path::{Path, PathBuf};

use crate::core::ids::PackId;
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::fs::tree::{self, TreeLimits, TreeSnapshot};

/// Single-file limit (PACK_SPEC.md §3).
pub const STORE_FILE_BYTES_LIMIT: u64 = 50 * 1024 * 1024;

/// Total pack-size limit (PACK_SPEC.md §3).
pub const STORE_TOTAL_LIMIT: u64 = 200 * 1024 * 1024;
/// File-count limit (PACK_SPEC.md §3).
pub const STORE_FILE_LIMIT: usize = 2000;
/// Path-depth limit (PACK_SPEC.md §2).
pub const STORE_DEPTH_LIMIT: usize = 8;

/// The final directory for a pack.
#[must_use]
pub fn pack_directory(packs_dir: &Path, pack_id: &PackId) -> PathBuf {
    packs_dir.join(pack_id.as_str())
}

/// Copies a validated pack tree into the store.
///
/// `expected_hash` is the content hash from validation. The destination must not exist.
///
/// # Errors
///
/// Returns `E_TARGET_EXISTS` when the pack directory already exists, `E_CHECKSUM_MISMATCH`
/// when the copied tree differs from the validated one, and a typed filesystem error
/// otherwise. A failed copy leaves no staging directory behind.
pub fn install_tree(
    source: &Path,
    packs_dir: &Path,
    pack_id: &PackId,
    expected_hash: &str,
) -> Result<TreeSnapshot> {
    std::fs::create_dir_all(packs_dir)
        .map_err(|error| crate::fs::sandbox::io_error(packs_dir, &error))?;
    set_private_directory(packs_dir)?;

    let destination = pack_directory(packs_dir, pack_id);
    if std::fs::symlink_metadata(&destination).is_ok() {
        return Err(MorphixError::new(
            ErrorCode::TargetExists,
            "a pack directory with this ID already exists",
        )
        .with_context("pack_id", pack_id.as_str()));
    }

    let staging = packs_dir.join(format!(
        ".staging-{}-{}",
        pack_id.as_str(),
        std::process::id()
    ));
    if std::fs::symlink_metadata(&staging).is_ok() {
        return Err(MorphixError::new(
            ErrorCode::Busy,
            "another install left a staging directory behind",
        )
        .with_context("path", staging.display().to_string()));
    }

    let limits = TreeLimits {
        max_files: Some(STORE_FILE_LIMIT),
        max_total_bytes: Some(STORE_TOTAL_LIMIT),
        max_file_bytes: Some(STORE_FILE_BYTES_LIMIT),
        max_depth: Some(STORE_DEPTH_LIMIT),
    };

    let snapshot = match tree::copy_tree_no_follow(source, &staging, limits) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            remove_staging(&staging)?;
            return Err(error);
        }
    };

    if snapshot.content_hash() != expected_hash {
        remove_staging(&staging)?;
        return Err(MorphixError::new(
            ErrorCode::ChecksumMismatch,
            "the pack changed while it was being installed",
        )
        .with_context("pack_id", pack_id.as_str()));
    }

    if let Err(error) = std::fs::rename(&staging, &destination) {
        let mapped = crate::fs::sandbox::io_error(&destination, &error);
        remove_staging(&staging)?;
        return Err(if std::fs::symlink_metadata(&destination).is_ok() {
            MorphixError::new(
                ErrorCode::TargetExists,
                "a pack directory with this ID appeared during installation",
            )
            .with_context("pack_id", pack_id.as_str())
        } else {
            mapped
        });
    }

    tree::sync_dir(packs_dir)?;
    Ok(snapshot)
}

/// Removes an installed pack directory that this engine created.
///
/// Used only to undo an install whose database registration failed. It never follows
/// symlinks and refuses a non-directory.
///
/// # Errors
///
/// Returns a typed filesystem error when the directory cannot be removed.
pub fn remove_installed(packs_dir: &Path, pack_id: &PackId) -> Result<()> {
    let destination = pack_directory(packs_dir, pack_id);
    match std::fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            tree::remove_tree(&destination)
        }
        Ok(_) => Err(MorphixError::new(
            ErrorCode::FileTypeRejected,
            "the pack store entry is not a directory",
        )
        .with_context("path", destination.display().to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(crate::fs::sandbox::io_error(&destination, &error)),
    }
}

fn remove_staging(staging: &Path) -> Result<()> {
    if std::fs::symlink_metadata(staging).is_err() {
        return Ok(());
    }
    tree::remove_tree(staging).map_err(|error| {
        MorphixError::new(error.code, error.message)
            .with_context("staging_cleanup", staging.display().to_string())
    })
}

fn set_private_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| crate::fs::sandbox::io_error(path, &error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::path::Path;

    use super::{install_tree, pack_directory, remove_installed};
    use crate::core::ids::PackId;
    use crate::errors::ErrorCode;
    use crate::fs::tree;

    fn write(path: &Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, contents).expect("write");
    }

    fn source(root: &Path) {
        write(&root.join("pack.toml"), b"schema_version = 1\n");
        write(&root.join("README.md"), b"# Pack\n");
        write(&root.join("themes/a.txt"), b"theme\n");
    }

    #[test]
    fn installation_copies_and_matches_the_validated_hash() {
        let temp = tempfile::tempdir().expect("temp");
        let source_dir = temp.path().join("source");
        source(&source_dir);
        let expected = tree::snapshot(&source_dir)
            .expect("snapshot")
            .content_hash();
        let packs = temp.path().join("packs");
        let id = PackId::parse("dev.example.one").expect("id");

        let installed = install_tree(&source_dir, &packs, &id, &expected).expect("install");
        assert_eq!(installed.content_hash(), expected);
        assert!(pack_directory(&packs, &id).join("pack.toml").is_file());

        let error = install_tree(&source_dir, &packs, &id, &expected).expect_err("duplicate");
        assert_eq!(error.code, ErrorCode::TargetExists);
    }

    #[test]
    fn a_changed_tree_is_refused_and_leaves_no_staging() {
        let temp = tempfile::tempdir().expect("temp");
        let source_dir = temp.path().join("source");
        source(&source_dir);
        let expected = tree::snapshot(&source_dir)
            .expect("snapshot")
            .content_hash();
        write(
            &source_dir.join("themes/a.txt"),
            b"changed after validation\n",
        );
        let packs = temp.path().join("packs");
        let id = PackId::parse("dev.example.two").expect("id");

        let error = install_tree(&source_dir, &packs, &id, &expected).expect_err("mismatch");
        assert_eq!(error.code, ErrorCode::ChecksumMismatch);
        assert!(!pack_directory(&packs, &id).exists());
        let staging = std::fs::read_dir(&packs)
            .expect("packs")
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
            .count();
        assert_eq!(staging, 0);
    }

    #[test]
    fn removing_an_installed_tree_is_idempotent() {
        let temp = tempfile::tempdir().expect("temp");
        let source_dir = temp.path().join("source");
        source(&source_dir);
        let expected = tree::snapshot(&source_dir)
            .expect("snapshot")
            .content_hash();
        let packs = temp.path().join("packs");
        let id = PackId::parse("dev.example.three").expect("id");
        install_tree(&source_dir, &packs, &id, &expected).expect("install");
        remove_installed(&packs, &id).expect("remove");
        remove_installed(&packs, &id).expect("remove again");
        assert!(!pack_directory(&packs, &id).exists());
    }
}
