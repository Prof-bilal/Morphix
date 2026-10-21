//! Filesystem primitives: the `FileSystem` trait, path sandboxing, atomic writes, tree
//! hashing/copy/removal, the mutation lock, and temp trees for tests.
//!
//! **Ownership.** Every filesystem operation in the product goes through here, so tests
//! can substitute a fake or a recording implementation (TESTING.md §3).
//!
//! **Allowed dependencies:** `errors`.
//! **Must never depend on:** `packs`, `desktop`, `storage`, `planning` (ARCHITECTURE.md §5).
//!
//! **Filled in by:** P4-01 (`mod.rs` trait + real implementation), P4-02
//! (`atomic.rs` + the `write_file_atomic` trait method), P4-03 (`sandbox.rs`),
//! P4-04 (`tree.rs`), P4-13 (`lock.rs`), `temp.rs`.

use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use crate::errors::{ErrorCode, MorphixError, Result};

pub mod atomic;
pub mod lock;
pub mod sandbox;
pub mod tree;

use self::sandbox::{SafePath, io_error};
use self::tree::EntryKind;

/// Default ceiling for a single snapshot file read (TRANSACTIONS.md §6: whole config
/// files are captured, but a runaway file must fail fast instead of exhausting memory).
pub const MAX_READ_BYTES: u64 = 16 * 1024 * 1024;

/// Metadata observed without following a final-component symlink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    /// Entry kind, as observed without following links.
    pub kind: EntryKind,
    /// Size in bytes (0 for directories).
    pub len: u64,
    /// Permission bits.
    pub mode: u32,
}

/// One directory entry, with its kind observed without following links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// File name within the directory (UTF-8; anything else is refused at listing).
    pub name: String,
    /// Entry kind, as observed without following links.
    pub kind: EntryKind,
}

/// Every filesystem read in the product goes through this trait, so purity and failure
/// tests can substitute a recording or fault-injecting implementation.
///
/// All observations are no-follow: symlinks are reported or refused, never traversed.
/// Callers pass paths as [`SafePath`], which already rejects relative paths, `..`, and
/// NUL bytes before any syscall.
pub trait FileSystem {
    /// Reads a whole file, refusing symlinks and directories.
    ///
    /// # Errors
    ///
    /// Returns `E_SYMLINK_TARGET` for a symlinked final component, `E_CONFIG_TOO_LARGE`
    /// when `max_bytes` is exceeded, and a typed I/O error otherwise.
    fn read_file(&self, path: &SafePath, max_bytes: u64) -> Result<Vec<u8>>;

    /// Returns metadata observed with `symlink_metadata` (never following links).
    ///
    /// # Errors
    ///
    /// Returns a typed I/O error when the path cannot be inspected.
    fn metadata(&self, path: &SafePath) -> Result<FileMetadata>;

    /// Lists directory entries sorted by name, with kinds observed without following
    /// links. The listing itself never refuses symlinks: it reports them so the caller
    /// can reject them with the right code.
    ///
    /// # Errors
    ///
    /// Returns a typed I/O error when the directory cannot be listed, and
    /// `E_INVALID_PATH` for a non-UTF-8 file name.
    fn list_dir(&self, path: &SafePath) -> Result<Vec<DirEntry>>;

    /// Replaces or creates a file atomically with an explicit mode.
    ///
    /// Same-directory temp file, `fsync`, rename, directory `fsync`; no temp residue
    /// remains on success, and the temp file is removed on failure. The caller names
    /// the mode explicitly: use [`atomic::current_mode`] to preserve an existing
    /// file's mode.
    ///
    /// # Errors
    ///
    /// Returns `E_INSUFFICIENT_SPACE` on `ENOSPC` and a typed I/O error otherwise.
    fn write_file_atomic(&self, path: &SafePath, bytes: &[u8], mode: u32) -> Result<()>;

    /// Creates a directory and its missing parents, then sets the mode on the final
    /// directory. Missing parents keep their ambient modes; the caller picks a
    /// private mode for the store roots it owns.
    ///
    /// # Errors
    ///
    /// Returns a typed I/O error when the directory cannot be created.
    fn create_dir_all(&self, path: &SafePath, mode: u32) -> Result<()>;
}

/// The production [`FileSystem`]: direct `std::fs` calls with no caching, no retries,
/// and no symlink following.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn read_file(&self, path: &SafePath, max_bytes: u64) -> Result<Vec<u8>> {
        let metadata = fs::symlink_metadata(path.as_path())
            .map_err(|error| io_error(path.as_path(), &error))?;
        if metadata.file_type().is_symlink() {
            return Err(MorphixError::new(
                ErrorCode::SymlinkTarget,
                "Morphix refuses to read through a symbolic link",
            )
            .with_context("path", path.as_path().display().to_string()));
        }
        if metadata.is_dir() {
            return Err(
                MorphixError::new(ErrorCode::Io, "a filesystem operation failed")
                    .with_context("path", path.as_path().display().to_string())
                    .with_context("io_kind", "IsADirectory"),
            );
        }
        let file =
            fs::File::open(path.as_path()).map_err(|error| io_error(path.as_path(), &error))?;
        // `take(max + 1)` detects an overrun even when the size changed after metadata.
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| io_error(path.as_path(), &error))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
            return Err(MorphixError::new(
                ErrorCode::ConfigTooLarge,
                "the file exceeds the read cap",
            )
            .with_context("path", path.as_path().display().to_string())
            .with_context("max_bytes", max_bytes.to_string()));
        }
        Ok(bytes)
    }

    fn metadata(&self, path: &SafePath) -> Result<FileMetadata> {
        let metadata = fs::symlink_metadata(path.as_path())
            .map_err(|error| io_error(path.as_path(), &error))?;
        Ok(FileMetadata {
            kind: EntryKind::from_file_type(&metadata.file_type()),
            len: metadata.len(),
            mode: metadata.mode(),
        })
    }

    fn list_dir(&self, path: &SafePath) -> Result<Vec<DirEntry>> {
        let read_dir =
            fs::read_dir(path.as_path()).map_err(|error| io_error(path.as_path(), &error))?;
        let mut entries = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|error| io_error(path.as_path(), &error))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|name| invalid_name(path.as_path(), &name))?;
            let file_type = entry
                .file_type()
                .map_err(|error| io_error(path.as_path(), &error))?;
            entries.push(DirEntry {
                name,
                kind: EntryKind::from_file_type(&file_type),
            });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    fn write_file_atomic(&self, path: &SafePath, bytes: &[u8], mode: u32) -> Result<()> {
        atomic::write_file_atomic(path.as_path(), bytes, mode)
    }

    fn create_dir_all(&self, path: &SafePath, mode: u32) -> Result<()> {
        fs::create_dir_all(path.as_path()).map_err(|error| io_error(path.as_path(), &error))?;
        fs::set_permissions(path.as_path(), fs::Permissions::from_mode(mode))
            .map_err(|error| io_error(path.as_path(), &error))
    }
}

fn invalid_name(directory: &std::path::Path, name: &std::ffi::OsString) -> MorphixError {
    MorphixError::new(
        ErrorCode::InvalidPath,
        "the directory contains a non-UTF-8 file name",
    )
    .with_context("path", directory.display().to_string())
    .with_context("name", name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;

    use super::{EntryKind, FileSystem, RealFileSystem};
    use crate::errors::ErrorCode;
    use crate::fs::sandbox::SafePath;

    fn fixture() -> (tempfile::TempDir, SafePath) {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = SafePath::new(directory.path().join("root")).expect("safe root");
        std::fs::create_dir(root.as_path()).expect("create root");
        (directory, root)
    }

    #[test]
    fn read_metadata_and_list_round_trip() {
        let (_directory, root) = fixture();
        let fs = RealFileSystem;
        std::fs::write(root.as_path().join("b.txt"), b"bbb").expect("write b");
        std::fs::write(root.as_path().join("a.txt"), b"a").expect("write a");
        std::fs::create_dir(root.as_path().join("sub")).expect("create sub");

        let bytes = fs
            .read_file(
                &SafePath::new(root.as_path().join("a.txt")).expect("safe"),
                1024,
            )
            .expect("read");
        assert_eq!(bytes, b"a");

        let metadata = fs
            .metadata(&SafePath::new(root.as_path().join("a.txt")).expect("safe"))
            .expect("metadata");
        assert_eq!(metadata.kind, EntryKind::File);
        assert_eq!(metadata.len, 1);

        let directory = fs.metadata(&root).expect("dir metadata");
        assert_eq!(directory.kind, EntryKind::Directory);

        let listing = fs.list_dir(&root).expect("list");
        let names: Vec<&str> = listing.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["a.txt", "b.txt", "sub"]);
    }

    #[test]
    fn symlinks_are_refused_on_read_and_reported_on_list() {
        let (_directory, root) = fixture();
        let fs = RealFileSystem;
        std::fs::write(root.as_path().join("real.txt"), b"real").expect("write real");
        std::os::unix::fs::symlink(
            root.as_path().join("real.txt"),
            root.as_path().join("link.txt"),
        )
        .expect("symlink");

        let error = fs
            .read_file(
                &SafePath::new(root.as_path().join("link.txt")).expect("safe"),
                1024,
            )
            .expect_err("symlink read must fail");
        assert_eq!(error.code, ErrorCode::SymlinkTarget);

        let metadata = fs
            .metadata(&SafePath::new(root.as_path().join("link.txt")).expect("safe"))
            .expect("symlink metadata");
        assert_eq!(metadata.kind, EntryKind::Symlink);

        let listing = fs.list_dir(&root).expect("list");
        let link = listing
            .iter()
            .find(|entry| entry.name == "link.txt")
            .expect("link listed");
        assert_eq!(link.kind, EntryKind::Symlink);
    }

    #[test]
    fn missing_paths_map_to_typed_io_errors() {
        let (_directory, root) = fixture();
        let fs = RealFileSystem;
        let missing = SafePath::new(root.as_path().join("missing.txt")).expect("safe");

        let error = fs.read_file(&missing, 1024).expect_err("missing read");
        assert_eq!(error.code, ErrorCode::Io);
        let error = fs.metadata(&missing).expect_err("missing metadata");
        assert_eq!(error.code, ErrorCode::Io);
    }

    #[test]
    fn directories_are_not_readable_as_files() {
        let (_directory, root) = fixture();
        let fs = RealFileSystem;
        let error = fs.read_file(&root, 1024).expect_err("dir read");
        assert_eq!(error.code, ErrorCode::Io);
    }

    #[test]
    fn over_cap_reads_fail_fast_with_config_too_large() {
        let (_directory, root) = fixture();
        let fs = RealFileSystem;
        std::fs::write(root.as_path().join("big.bin"), b"0123456789").expect("write big");
        let error = fs
            .read_file(
                &SafePath::new(root.as_path().join("big.bin")).expect("safe"),
                4,
            )
            .expect_err("over-cap read");
        assert_eq!(error.code, ErrorCode::ConfigTooLarge);
    }

    #[test]
    fn modes_are_observed() {
        let (_directory, root) = fixture();
        let fs = RealFileSystem;
        let file = root.as_path().join("mode.txt");
        std::fs::write(&file, b"x").expect("write");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).expect("chmod");
        let metadata = fs
            .metadata(&SafePath::new(&file).expect("safe"))
            .expect("metadata");
        assert_eq!(metadata.mode & 0o777, 0o640);
    }

    #[test]
    fn non_utf8_names_are_refused_at_listing() {
        let (_directory, root) = fixture();
        let fs = RealFileSystem;
        let raw = std::ffi::OsString::from_vec(vec![0xFF, 0xFE]);
        std::fs::write(root.as_path().join(&raw), b"x").expect("write raw");
        let error = fs.list_dir(&root).expect_err("non-UTF-8 listing");
        assert_eq!(error.code, ErrorCode::InvalidPath);
    }
}
