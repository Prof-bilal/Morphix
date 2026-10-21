//! Content-addressed object store: file blobs named by SHA-256, deduplicated across
//! snapshots, written with the atomic temp → fsync → rename → dir-fsync sequence.
//!
//! **Ownership.** The only writer of snapshot content. The database holds metadata and
//! the entry index; blobs live here, readable without SQLite (TRANSACTIONS.md §6.1,
//! DATABASE.md principle 2).
//!
//! **Layout.** `<root>/objects/<sha256>`, exactly as specified: flat, lowercase hex,
//! no subdirectories at V0.1 snapshot scale. The hash is validated before it ever
//! becomes a file name, so a crafted manifest cannot turn it into a path.

use crate::errors::{ErrorCode, MorphixError, Result};
use crate::fs::sandbox::SafePath;
use crate::fs::tree::sha256_hex;
use crate::fs::{FileSystem, MAX_READ_BYTES};

/// Private directory mode for the store root: blobs are copies of user config.
const STORE_DIR_MODE: u32 = 0o700;
/// Private file mode for blobs. Original modes live in the manifest entries.
const OBJECT_FILE_MODE: u32 = 0o600;

/// A validated object hash: exactly 64 lowercase hex characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectHash(String);

impl ObjectHash {
    /// Validates a hash received from a manifest or a caller.
    ///
    /// # Errors
    ///
    /// Returns `E_SNAPSHOT_CORRUPT` for anything that is not 64 lowercase hex
    /// characters. The manifest is untrusted input, so a bad hash is corruption, not
    /// a path error — and it is rejected before it can become a file name.
    pub fn parse(raw: &str) -> Result<Self> {
        let valid = raw.len() == 64
            && raw
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
        if !valid {
            return Err(MorphixError::new(
                ErrorCode::SnapshotCorrupt,
                "the object hash is not 64 lowercase hex characters",
            )
            .with_context("hash", raw));
        }
        Ok(Self(raw.to_owned()))
    }

    /// Computes the hash of in-memory bytes.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(sha256_hex(bytes))
    }

    /// The validated hex string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The snapshot object store rooted at `<root>/objects`.
pub struct ObjectStore<'a, F: FileSystem> {
    fs: &'a F,
    objects: SafePath,
}

impl<'a, F: FileSystem> ObjectStore<'a, F> {
    /// Opens (creating) the store under `root`. The `objects` directory is created
    /// with a private mode; missing parents keep their ambient modes.
    ///
    /// # Errors
    ///
    /// Returns a typed I/O error when the directory cannot be created.
    pub fn open(fs: &'a F, root: &SafePath) -> Result<Self> {
        let objects = root.join_relative("objects").map_err(|error| {
            MorphixError::new(ErrorCode::Internal, "the object store path is not safe")
                .with_context("reason", error.to_string())
        })?;
        fs.create_dir_all(&objects, STORE_DIR_MODE)?;
        Ok(Self { fs, objects })
    }

    /// Stores bytes and returns their hash. Identical content is written once: when
    /// the object already exists with matching content, nothing is rewritten.
    ///
    /// # Errors
    ///
    /// Returns a typed I/O error when the object cannot be written.
    pub fn put(&self, bytes: &[u8]) -> Result<ObjectHash> {
        let hash = ObjectHash::of_bytes(bytes);
        let path = self.object_path(&hash)?;
        match self.fs.metadata(&path) {
            Ok(_) => {
                // Present: confirm the bytes before sharing the blob. A mismatch here
                // means the store itself is damaged.
                let live = self.fs.read_file(&path, MAX_READ_BYTES)?;
                if live != bytes {
                    return Err(MorphixError::new(
                        ErrorCode::SnapshotCorrupt,
                        "the stored object does not match its hash",
                    )
                    .with_context("hash", hash.as_str()));
                }
            }
            Err(error) if error.code == ErrorCode::Io => {
                self.fs.write_file_atomic(&path, bytes, OBJECT_FILE_MODE)?;
            }
            Err(error) => return Err(error),
        }
        Ok(hash)
    }

    /// Reads a whole file (capped) and stores it, returning the hash.
    ///
    /// # Errors
    ///
    /// Returns the filesystem error when the file cannot be read, and the store
    /// error when the object cannot be written.
    pub fn put_file(&self, path: &SafePath) -> Result<ObjectHash> {
        let bytes = self.fs.read_file(path, MAX_READ_BYTES)?;
        self.put(&bytes)
    }

    /// Reads an object and verifies its content against the hash.
    ///
    /// # Errors
    ///
    /// Returns `E_SNAPSHOT_UNREADABLE` when the object is missing, and
    /// `E_SNAPSHOT_CORRUPT` when the bytes do not match the hash.
    pub fn get(&self, hash: &ObjectHash) -> Result<Vec<u8>> {
        let path = self.object_path(hash)?;
        let bytes = self.fs.read_file(&path, MAX_READ_BYTES).map_err(|error| {
            if error.code == ErrorCode::Io {
                MorphixError::new(
                    ErrorCode::SnapshotUnreadable,
                    "the snapshot object is missing",
                )
                .with_context("hash", hash.as_str())
            } else {
                error
            }
        })?;
        if ObjectHash::of_bytes(&bytes) != *hash {
            return Err(MorphixError::new(
                ErrorCode::SnapshotCorrupt,
                "the snapshot object does not match its hash",
            )
            .with_context("hash", hash.as_str()));
        }
        Ok(bytes)
    }

    /// Whether the object is present (without verifying content).
    pub fn contains(&self, hash: &ObjectHash) -> bool {
        self.object_path(hash)
            .and_then(|path| self.fs.metadata(&path))
            .is_ok()
    }

    fn object_path(&self, hash: &ObjectHash) -> Result<SafePath> {
        self.objects.join_relative(hash.as_str()).map_err(|error| {
            MorphixError::new(ErrorCode::Internal, "a validated hash is a safe path")
                .with_context("reason", error.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{ObjectHash, ObjectStore};
    use crate::errors::ErrorCode;
    use crate::fs::sandbox::SafePath;
    use crate::fs::{FileSystem, RealFileSystem};

    fn store<'a>(
        fs: &'a RealFileSystem,
        directory: &'a tempfile::TempDir,
    ) -> ObjectStore<'a, RealFileSystem> {
        let root = SafePath::new(directory.path().join("snapshots")).expect("safe root");
        ObjectStore::open(fs, &root).expect("open store")
    }

    #[test]
    fn put_and_get_round_trip() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let store = store(&fs, &directory);

        let hash = store.put(b"[General]\n").expect("put");
        assert_eq!(hash, ObjectHash::of_bytes(b"[General]\n"));
        assert!(store.contains(&hash));
        assert_eq!(store.get(&hash).expect("get"), b"[General]\n");
    }

    #[test]
    fn identical_content_is_stored_once() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let store = store(&fs, &directory);

        let first = store.put(b"shared").expect("first put");
        let second = store.put(b"shared").expect("second put");
        assert_eq!(first, second);

        let objects = directory.path().join("snapshots/objects");
        let count = std::fs::read_dir(&objects).expect("list").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn put_file_reads_through_the_filesystem() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let store = store(&fs, &directory);
        let source = SafePath::new(directory.path().join("kdeglobals")).expect("safe");
        fs.write_file_atomic(&source, b"data", 0o644)
            .expect("write");

        let hash = store.put_file(&source).expect("put file");
        assert_eq!(store.get(&hash).expect("get"), b"data");
    }

    #[test]
    fn missing_objects_are_unreadable_not_generic_io() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let store = store(&fs, &directory);
        let hash = ObjectHash::of_bytes(b"absent");
        assert!(!store.contains(&hash));
        assert_eq!(
            store.get(&hash).expect_err("missing").code,
            ErrorCode::SnapshotUnreadable
        );
    }

    #[test]
    fn tampered_objects_are_corrupt() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let store = store(&fs, &directory);
        let hash = store.put(b"original").expect("put");

        // Tamper behind the store's back.
        let objects = SafePath::new(directory.path().join("snapshots/objects")).expect("safe");
        let path = objects.join_relative(hash.as_str()).expect("object path");
        fs.write_file_atomic(&path, b"tampered", 0o600)
            .expect("tamper");

        assert_eq!(
            store.get(&hash).expect_err("tampered").code,
            ErrorCode::SnapshotCorrupt
        );
    }

    #[test]
    fn malformed_hashes_never_become_file_names() {
        for raw in [
            "../escape",
            "../../etc/passwd",
            "ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            "short",
            "",
            "zz5623fb1a8b2b2e8e0f3c4a5b6d7e8f9012345678901234567890123456789",
        ] {
            assert!(
                ObjectHash::parse(raw).is_err(),
                "must reject {raw:?} before it becomes a path"
            );
        }
        let valid = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(ObjectHash::parse(valid).expect("valid").as_str(), valid);
        // Uppercase hex is rejected: hashes are canonical lowercase only.
        assert!(ObjectHash::parse(&valid.to_uppercase()).is_err());
    }
}
