//! Atomic file writes: same-directory temp file, fsync, rename, directory fsync.
//!
//! **Ownership.** The only way the product creates or replaces a file, so a crash can
//! never leave a half-written target behind (TRANSACTIONS.md §6: objects and the
//! manifest follow temp → fsync → rename → dir-fsync). Mode preservation is explicit:
//! the caller names the mode, and an existing file keeps its mode unless the caller
//! passes a new one.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::errors::{ErrorCode, MorphixError, Result};

use super::sandbox::io_error;

/// Counter that disambiguates temp names inside one process.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to `path` atomically with the given permission bits.
///
/// The sequence is: create a same-directory temp file (`O_EXCL`), write, set the mode,
/// `fsync` the file, `rename` over the target, `fsync` the directory. The temp file is
/// removed on any failure; on success no temp residue remains.
///
/// # Errors
///
/// Returns `E_INSUFFICIENT_SPACE` when the filesystem reports `ENOSPC`, a typed I/O
/// error otherwise, and `E_INVALID_PATH` for a path with no parent directory.
pub fn write_file_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        MorphixError::new(ErrorCode::InvalidPath, "the path has no parent directory")
            .with_context("path", path.display().to_string())
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| invalid_target(path, "the path has no file name"))?;
    let mut temp_path = parent.join(format!(
        ".{}.morphix-tmp.{}",
        file_name.to_string_lossy(),
        unique_suffix()
    ));
    // `create_new` fails instead of truncating, so a stale temp from a crashed earlier
    // write can never be silently reused.
    let mut attempts = 0;
    let mut file = loop {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => break file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempts < 8 => {
                attempts += 1;
                temp_path = parent.join(format!(
                    ".{}.morphix-tmp.{attempts}",
                    file_name.to_string_lossy()
                ));
            }
            Err(error) => return Err(map_write_error(path, &error)),
        }
    };
    let result = write_and_commit(&mut file, &temp_path, path, parent, bytes, mode);
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn write_and_commit(
    file: &mut fs::File,
    temp_path: &Path,
    target: &Path,
    parent: &Path,
    bytes: &[u8],
    mode: u32,
) -> Result<()> {
    file.write_all(bytes)
        .map_err(|error| map_write_error(target, &error))?;
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(|error| map_write_error(target, &error))?;
    file.sync_all()
        .map_err(|error| map_write_error(target, &error))?;
    // The file was synced above; on Unix the rename below is atomic whether or not
    // this handle is still open, so there is nothing left to flush or close here.
    fs::rename(temp_path, target).map_err(|error| map_write_error(target, &error))?;
    sync_dir(parent)
}

/// Persists a directory entry change (rename, create, remove) to stable storage.
pub fn sync_dir(path: &Path) -> Result<()> {
    let directory = fs::File::open(path).map_err(|error| super::sandbox::io_error(path, &error))?;
    directory
        .sync_all()
        .map_err(|error| super::sandbox::io_error(path, &error))
}

/// Reads the permission bits of an existing file, so a rewrite can preserve them.
pub fn current_mode(path: &Path) -> Result<u32> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| super::sandbox::io_error(path, &error))?;
    Ok(metadata.mode() & 0o7777)
}

fn unique_suffix() -> String {
    let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}.{count}", std::process::id())
}

/// `ENOSPC` has a dedicated code so operators can grep for a full disk; every other
/// failure keeps the generic typed mapping.
fn map_write_error(path: &Path, error: &std::io::Error) -> MorphixError {
    if error.kind() == std::io::ErrorKind::StorageFull {
        return MorphixError::new(
            ErrorCode::InsufficientSpace,
            "the filesystem is full; nothing was written",
        )
        .with_context("path", path.display().to_string())
        .with_context("io_kind", format!("{:?}", error.kind()));
    }
    io_error(path, error)
}

fn invalid_target(path: &Path, reason: &'static str) -> MorphixError {
    MorphixError::new(ErrorCode::InvalidPath, "the write target is not safe")
        .with_context("path", path.display().to_string())
        .with_context("reason", reason)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::os::unix::fs::PermissionsExt;

    use super::{current_mode, write_file_atomic};

    #[test]
    fn round_trip_applies_the_given_mode() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("kdeglobals");
        write_file_atomic(&target, b"[General]\n", 0o600).expect("write");
        assert_eq!(std::fs::read(&target).expect("read"), b"[General]\n");
        assert_eq!(current_mode(&target).expect("mode") & 0o777, 0o600);
    }

    #[test]
    fn rewrite_preserves_nothing_by_itself_but_applies_the_passed_mode() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("target.txt");
        write_file_atomic(&target, b"one", 0o640).expect("write one");
        let before = current_mode(&target).expect("mode before");
        write_file_atomic(&target, b"two", before).expect("write two");
        assert_eq!(std::fs::read(&target).expect("read"), b"two");
        assert_eq!(current_mode(&target).expect("mode after") & 0o777, 0o640);
    }

    #[test]
    fn no_temp_residue_remains_after_success() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("target.txt");
        write_file_atomic(&target, b"data", 0o644).expect("write");
        let leftovers: Vec<_> = std::fs::read_dir(directory.path())
            .expect("list")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains("morphix-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp residue: {leftovers:?}");
    }

    #[test]
    fn missing_parent_is_a_typed_error() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("no-such-dir/target.txt");
        let error = write_file_atomic(&target, b"data", 0o644).expect_err("missing parent");
        assert_eq!(error.code, crate::errors::ErrorCode::Io);
    }

    #[test]
    fn parentless_paths_are_refused() {
        let error =
            write_file_atomic(std::path::Path::new(""), b"data", 0o644).expect_err("empty path");
        assert_eq!(error.code, crate::errors::ErrorCode::InvalidPath);
    }

    #[test]
    fn permissions_come_from_the_caller_not_the_umask() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("secret.txt");
        write_file_atomic(&target, b"secret", 0o600).expect("write");
        let mode = std::fs::metadata(&target)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
