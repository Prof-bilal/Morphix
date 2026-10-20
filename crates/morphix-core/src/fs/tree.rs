//! Deterministic tree inspection, hashing, and no-follow copying.
//!
//! **Ownership.** The low-level filesystem tree primitives used by pack validation and the
//! pack store. Walks always use `symlink_metadata` and never follow a symlink. Phase 4
//! extends this module with owned tree removal and restore support.
//!
//! The content hash covers paths, kinds, file modes, sizes, and file SHA-256 values in
//! sorted order, so two identical trees produce the same hash regardless of directory
//! iteration order. Directory permission bits are normalized and deliberately excluded:
//! an installed copy uses safe `0755` directories rather than preserving pack-supplied
//! modes.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::errors::{ErrorCode, MorphixError, Result};

use super::sandbox::io_error;

/// Kind of a tree entry, as observed without following links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link (rejected by pack validation).
    Symlink,
    /// A FIFO, socket, device, or other non-regular entry.
    Other,
}

/// One inspected tree entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    /// Path relative to the root, using `/` separators.
    pub relative: String,
    /// Observed entry kind.
    pub kind: EntryKind,
    /// Size in bytes (0 for directories).
    pub size: u64,
    /// Permission bits.
    pub mode: u32,
    /// Hard-link count.
    pub links: u64,
    /// SHA-256 of the file content, when this is a regular file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

/// A deterministic snapshot of a directory tree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeSnapshot {
    /// Entries sorted by relative path. The root itself is not included.
    pub entries: Vec<TreeEntry>,
    /// Sum of file sizes.
    pub total_bytes: u64,
}

impl TreeSnapshot {
    /// Number of files in the snapshot.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::File)
            .count()
    }

    /// Looks up a relative path.
    #[must_use]
    pub fn entry(&self, relative: &str) -> Option<&TreeEntry> {
        self.entries
            .binary_search_by(|entry| entry.relative.as_str().cmp(relative))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Files keyed by relative path, for checksum verification.
    #[must_use]
    pub fn files(&self) -> BTreeMap<&str, &TreeEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::File)
            .map(|entry| (entry.relative.as_str(), entry))
            .collect()
    }

    /// A stable, diffable text rendering: `kind mode hash size path`.
    #[must_use]
    pub fn render(&self) -> String {
        let mut output = String::new();
        for entry in &self.entries {
            let hash = entry.hash.as_deref().unwrap_or("-");
            output.push_str(&format!(
                "{:?}\t{:04o}\t{}\t{}\t{}\n",
                entry.kind, entry.mode, entry.links, hash, entry.relative
            ));
        }
        output
    }

    /// The deterministic content hash for this snapshot.
    #[must_use]
    pub fn content_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"morphix-tree-v1\0");
        for entry in &self.entries {
            let kind = match entry.kind {
                EntryKind::File => b'f',
                EntryKind::Directory => b'd',
                EntryKind::Symlink => b'l',
                EntryKind::Other => b'o',
            };
            hasher.update([kind]);
            hasher.update((entry.relative.len() as u64).to_le_bytes());
            hasher.update(entry.relative.as_bytes());
            let mode = if entry.kind == EntryKind::File {
                entry.mode
            } else {
                0
            };
            hasher.update(mode.to_le_bytes());
            hasher.update(entry.size.to_le_bytes());
            hasher.update(entry.hash.as_deref().unwrap_or("").as_bytes());
        }
        hex(&hasher.finalize())
    }
}

/// Optional limits applied while copying a tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeLimits {
    /// Maximum number of files.
    pub max_files: Option<usize>,
    /// Maximum total bytes.
    pub max_total_bytes: Option<u64>,
    /// Maximum size of one file.
    pub max_file_bytes: Option<u64>,
    /// Maximum directory depth (the root has depth 0).
    pub max_depth: Option<usize>,
}

impl TreeLimits {
    /// No limits.
    #[must_use]
    pub fn unlimited() -> Self {
        Self::default()
    }
}

/// Inspects a tree without following symlinks.
///
/// # Errors
///
/// Returns `E_INVALID_PATH` for a non-UTF-8 name, and a typed I/O error when the tree
/// cannot be read. Symlinks are reported as entries, not followed.
pub fn snapshot(root: &Path) -> Result<TreeSnapshot> {
    let mut entries = Vec::new();
    let mut context = WalkContext {
        root,
        entries: &mut entries,
        total_bytes: 0,
    };
    context.walk(root)?;
    let total_bytes = context.total_bytes;
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(TreeSnapshot {
        entries,
        total_bytes,
    })
}

struct WalkContext<'a> {
    root: &'a Path,
    entries: &'a mut Vec<TreeEntry>,
    total_bytes: u64,
}

impl WalkContext<'_> {
    fn walk(&mut self, directory: &Path) -> Result<()> {
        let mut children = read_dir_sorted(directory)?;
        for path in children.drain(..) {
            let relative = relative_name(self.root, &path)?;
            let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, &error))?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                self.entries
                    .push(entry(relative, EntryKind::Symlink, &metadata, None));
            } else if file_type.is_dir() {
                self.entries
                    .push(entry(relative, EntryKind::Directory, &metadata, None));
                self.walk(&path)?;
            } else if file_type.is_file() {
                let hash = hash_file(&path)?;
                let size = metadata.len();
                self.total_bytes = self.total_bytes.saturating_add(size);
                self.entries
                    .push(entry(relative, EntryKind::File, &metadata, Some(hash)));
            } else {
                self.entries
                    .push(entry(relative, EntryKind::Other, &metadata, None));
            }
        }
        Ok(())
    }
}

/// Copies a tree without following symlinks or hardlinks.
///
/// `destination` must not exist; it is created as a directory. The caller owns cleanup on
/// failure. Limits are enforced before a file is opened and while it is copied. Every file
/// is fsynced, and every directory is fsynced before returning.
///
/// # Errors
///
/// Returns `E_SYMLINK_REJECTED` for symlinks and hardlinks, `E_FILE_TYPE_REJECTED` for
/// non-regular entries or executable bits, `E_LIMIT_EXCEEDED` for a configured limit, and
/// typed I/O errors for filesystem failures.
pub fn copy_tree_no_follow(
    source: &Path,
    destination: &Path,
    limits: TreeLimits,
) -> Result<TreeSnapshot> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(MorphixError::new(
            ErrorCode::TargetExists,
            "the copy destination already exists",
        )
        .with_context("path", destination.display().to_string()));
    }
    fs::create_dir(destination).map_err(|error| io_error(destination, &error))?;
    set_mode(destination, 0o755)?;

    let mut entries = Vec::new();
    let mut context = CopyContext {
        source,
        limits,
        entries: &mut entries,
        total_bytes: 0,
    };
    context.walk(source, destination, 0)?;
    let total_bytes = context.total_bytes;
    sync_dir(destination)?;
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(TreeSnapshot {
        entries,
        total_bytes,
    })
}

struct CopyContext<'a> {
    source: &'a Path,
    limits: TreeLimits,
    entries: &'a mut Vec<TreeEntry>,
    total_bytes: u64,
}

impl CopyContext<'_> {
    fn walk(&mut self, source: &Path, destination: &Path, depth: usize) -> Result<()> {
        if self.limits.max_depth.is_some_and(|limit| depth > limit) {
            return Err(limit_error("the tree is deeper than the configured limit"));
        }

        let children = read_dir_sorted(source)?;
        for path in children {
            let relative = relative_name(self.source, &path)?;
            let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, &error))?;
            let file_type = metadata.file_type();

            if file_type.is_symlink() {
                return Err(symlink_error(&path, "the tree contains a symbolic link"));
            }

            let target = destination.join(
                Path::new(&relative)
                    .file_name()
                    .ok_or_else(|| invalid_tree_path(&relative))?,
            );

            if file_type.is_dir() {
                fs::create_dir(&target).map_err(|error| io_error(&target, &error))?;
                set_mode(&target, 0o755)?;
                self.entries.push(entry(
                    relative.clone(),
                    EntryKind::Directory,
                    &metadata,
                    None,
                ));
                self.walk(&path, &target, depth + 1)?;
                sync_dir(&target)?;
            } else if file_type.is_file() {
                // Directories always have link counts above one (their `..` entries);
                // only a regular file with extra links is a hardlink to reject.
                if metadata.nlink() > 1 {
                    return Err(symlink_error(&path, "the tree contains a hard link"));
                }
                if metadata.mode() & 0o111 != 0 {
                    return Err(MorphixError::new(
                        ErrorCode::FileTypeRejected,
                        "executable files are not allowed in a pack",
                    )
                    .with_context("path", relative));
                }
                if self
                    .limits
                    .max_file_bytes
                    .is_some_and(|limit| metadata.len() > limit)
                {
                    return Err(limit_error("a file is larger than the configured limit"));
                }
                let next_total = self.total_bytes.saturating_add(metadata.len());
                if self
                    .limits
                    .max_total_bytes
                    .is_some_and(|limit| next_total > limit)
                {
                    return Err(limit_error("the tree is larger than the configured limit"));
                }
                let files = self
                    .entries
                    .iter()
                    .filter(|entry| entry.kind == EntryKind::File)
                    .count();
                if self.limits.max_files.is_some_and(|limit| files >= limit) {
                    return Err(limit_error("the tree has more files than allowed"));
                }

                let hash = copy_file(&path, &target, metadata.len())?;
                set_mode(&target, metadata.mode() & 0o777)?;
                self.total_bytes = next_total;
                self.entries
                    .push(entry(relative, EntryKind::File, &metadata, Some(hash)));
            } else {
                return Err(MorphixError::new(
                    ErrorCode::FileTypeRejected,
                    "the tree contains a non-regular filesystem entry",
                )
                .with_context("path", relative));
            }
        }
        Ok(())
    }
}

/// Computes the SHA-256 of one file using a bounded buffer.
///
/// # Errors
///
/// Returns a typed I/O error when the file cannot be read.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|error| io_error(path, &error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(path, &error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Hashes bytes with SHA-256 and returns lowercase hex.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

/// Removes a file or directory tree. A symlink is removed as a link, never followed.
///
/// # Errors
///
/// Returns a typed I/O error when removal fails.
pub fn remove_tree(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            fs::remove_dir_all(path).map_err(|error| io_error(path, &error))
        }
        Ok(_) => fs::remove_file(path).map_err(|error| io_error(path, &error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path, &error)),
    }
}

/// fsyncs a directory so a completed rename or file creation is durable.
///
/// # Errors
///
/// Returns a typed I/O error when the directory cannot be opened or synced.
pub fn sync_dir(path: &Path) -> Result<()> {
    let directory = File::open(path).map_err(|error| io_error(path, &error))?;
    directory.sync_all().map_err(|error| io_error(path, &error))
}

fn copy_file(source: &Path, destination: &Path, expected_size: u64) -> Result<String> {
    let mut reader = File::open(source).map_err(|error| io_error(source, &error))?;
    let opened = reader
        .metadata()
        .map_err(|error| io_error(source, &error))?;
    if !opened.is_file() || opened.nlink() > 1 || opened.len() != expected_size {
        return Err(symlink_error(
            source,
            "the source changed while it was being copied",
        ));
    }

    let mut writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| io_error(destination, &error))?;

    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| io_error(source, &error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        writer
            .write_all(&buffer[..read])
            .map_err(|error| io_error(destination, &error))?;
    }
    writer
        .sync_all()
        .map_err(|error| io_error(destination, &error))?;
    Ok(hex(&hasher.finalize()))
}

fn read_dir_sorted(directory: &Path) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(directory).map_err(|error| io_error(directory, &error))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io_error(directory, &error))?;
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}

fn relative_name(root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| invalid_tree_path(&path.display().to_string()))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let name = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| invalid_tree_path(&path.display().to_string()))?;
        parts.push(name);
    }
    Ok(parts.join("/"))
}

fn entry(
    relative: String,
    kind: EntryKind,
    metadata: &fs::Metadata,
    hash: Option<String>,
) -> TreeEntry {
    TreeEntry {
        relative,
        kind,
        size: metadata.len(),
        mode: metadata.mode() & 0o777,
        links: metadata.nlink(),
        hash,
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| io_error(path, &error))
}

fn symlink_error(path: &Path, reason: &'static str) -> MorphixError {
    MorphixError::new(ErrorCode::SymlinkRejected, reason)
        .with_context("path", path.display().to_string())
}

fn invalid_tree_path(path: &str) -> MorphixError {
    MorphixError::new(
        ErrorCode::InvalidPath,
        "a tree entry has a path that cannot be represented",
    )
    .with_context("path", path)
}

fn limit_error(reason: &'static str) -> MorphixError {
    MorphixError::new(ErrorCode::LimitExceeded, reason)
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        output.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    output
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::os::unix::fs::PermissionsExt;

    use super::{EntryKind, TreeLimits, copy_tree_no_follow, remove_tree, sha256_hex, snapshot};
    use crate::errors::ErrorCode;

    fn write(path: &std::path::Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, contents).expect("write");
    }

    #[test]
    fn snapshots_are_sorted_and_hash_content() {
        let directory = tempfile::tempdir().expect("tempdir");
        write(&directory.path().join("b.txt"), b"two");
        write(&directory.path().join("a/one.txt"), b"one");

        let tree = snapshot(directory.path()).expect("snapshot");
        let paths: Vec<&str> = tree
            .entries
            .iter()
            .map(|entry| entry.relative.as_str())
            .collect();
        assert_eq!(paths, ["a", "a/one.txt", "b.txt"]);
        assert_eq!(tree.file_count(), 2);
        assert_eq!(tree.total_bytes, 6);
        assert_eq!(
            tree.entry("a/one.txt").and_then(|entry| entry.hash.clone()),
            Some(sha256_hex(b"one"))
        );
        assert_eq!(
            tree.content_hash(),
            snapshot(directory.path()).expect("again").content_hash()
        );
        assert!(tree.render().contains("a/one.txt"));
    }

    #[test]
    fn symlinks_are_reported_and_never_followed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let outside = directory.path().join("outside");
        write(&outside, b"secret");
        let root = directory.path().join("root");
        std::fs::create_dir(&root).expect("root");
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("symlink");

        let tree = snapshot(&root).expect("snapshot");
        assert_eq!(tree.entry("link").expect("link").kind, EntryKind::Symlink);
        assert_eq!(tree.total_bytes, 0, "the target must not be read");

        let copied = directory.path().join("copied");
        assert_eq!(
            copy_tree_no_follow(&root, &copied, TreeLimits::unlimited())
                .expect_err("symlink refused")
                .code,
            ErrorCode::SymlinkRejected
        );
        remove_tree(&copied).expect("cleanup");
    }

    #[test]
    fn copying_preserves_content_mode_and_rejects_executables() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("source");
        write(
            &source.join("themes/dark.colors"),
            b"[General]\nColorScheme=Dark\n",
        );
        std::fs::set_permissions(
            source.join("themes/dark.colors"),
            std::fs::Permissions::from_mode(0o640),
        )
        .expect("mode");

        let destination = directory.path().join("destination");
        let copied =
            copy_tree_no_follow(&source, &destination, TreeLimits::unlimited()).expect("copy");
        assert_eq!(copied.file_count(), 1);
        assert_eq!(
            std::fs::read(destination.join("themes/dark.colors")).expect("read"),
            b"[General]\nColorScheme=Dark\n"
        );
        assert_eq!(
            std::fs::metadata(destination.join("themes/dark.colors"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o640
        );

        let executable = directory.path().join("executable");
        write(&executable.join("install.sh"), b"#!/bin/sh\n");
        std::fs::set_permissions(
            executable.join("install.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("mode");
        assert_eq!(
            copy_tree_no_follow(
                &executable,
                &directory.path().join("executable-copy"),
                TreeLimits::unlimited()
            )
            .expect_err("executable refused")
            .code,
            ErrorCode::FileTypeRejected
        );
    }

    #[test]
    fn hardlinks_and_special_files_are_refused() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("source");
        write(&source.join("one"), b"one");
        std::fs::hard_link(source.join("one"), source.join("two")).expect("hard link");

        let error = copy_tree_no_follow(
            &source,
            &directory.path().join("copy"),
            TreeLimits::unlimited(),
        )
        .expect_err("hardlink refused");
        assert_eq!(error.code, ErrorCode::SymlinkRejected);
    }

    #[test]
    fn limits_are_enforced_before_copying_more() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("source");
        write(&source.join("one"), b"one");
        write(&source.join("two"), b"two");

        let error = copy_tree_no_follow(
            &source,
            &directory.path().join("copy"),
            TreeLimits {
                max_files: Some(1),
                ..TreeLimits::unlimited()
            },
        )
        .expect_err("file count");
        assert_eq!(error.code, ErrorCode::LimitExceeded);
    }

    #[test]
    fn remove_tree_removes_links_without_following() {
        let directory = tempfile::tempdir().expect("tempdir");
        let outside = directory.path().join("outside");
        write(&outside, b"keep");
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");

        remove_tree(&link).expect("remove link");
        assert!(!link.exists());
        assert!(outside.exists(), "the link target must survive");
    }
}
