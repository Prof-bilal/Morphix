//! A recording [`FileSystem`](morphix_core::fs::FileSystem) decorator for purity tests.
//!
//! The decorator delegates every call to the real filesystem and records the operation
//! and path, so a test can assert that a read-only phase (for example planning)
//! performed zero filesystem calls.

use std::cell::{Cell, RefCell};

use morphix_core::errors::{ErrorCode, MorphixError, Result};
use morphix_core::fs::sandbox::SafePath;
use morphix_core::fs::{DirEntry, FileMetadata, FileSystem, RealFileSystem};

/// Records every filesystem call and delegates to [`RealFileSystem`].
#[derive(Debug, Default)]
pub struct RecordingFileSystem {
    inner: RealFileSystem,
    calls: RefCell<Vec<String>>,
}

impl RecordingFileSystem {
    /// An empty recorder backed by the real filesystem.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The recorded calls in order, as `"operation:/absolute/path"`.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// The number of recorded filesystem calls.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.borrow().len()
    }

    fn record(&self, operation: &str, path: &SafePath) {
        self.calls
            .borrow_mut()
            .push(format!("{operation}:{}", path.as_path().display()));
    }
}

impl FileSystem for RecordingFileSystem {
    fn read_file(&self, path: &SafePath, max_bytes: u64) -> Result<Vec<u8>> {
        self.record("read_file", path);
        self.inner.read_file(path, max_bytes)
    }

    fn metadata(&self, path: &SafePath) -> Result<FileMetadata> {
        self.record("metadata", path);
        self.inner.metadata(path)
    }

    fn list_dir(&self, path: &SafePath) -> Result<Vec<DirEntry>> {
        self.record("list_dir", path);
        self.inner.list_dir(path)
    }

    fn write_file_atomic(&self, path: &SafePath, bytes: &[u8], mode: u32) -> Result<()> {
        self.record("write_file_atomic", path);
        self.inner.write_file_atomic(path, bytes, mode)
    }

    fn create_dir_all(&self, path: &SafePath, mode: u32) -> Result<()> {
        self.record("create_dir_all", path);
        self.inner.create_dir_all(path, mode)
    }
}

/// A fault-injecting filesystem for failure tests: wraps the recorder and fails
/// reads, file writes, or directory creation on demand with typed errors.
///
/// Faults stand in for conditions a temp-directory test cannot produce honestly
/// (a full disk) or cannot produce portably (refused permissions when running as
/// different users), while every other call reaches the real filesystem.
#[derive(Debug)]
pub struct FaultInjector {
    inner: RecordingFileSystem,
    fail_reads: Cell<bool>,
    fail_writes: Cell<bool>,
    fail_mkdir: Cell<bool>,
    failure_code: Cell<ErrorCode>,
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self {
            inner: RecordingFileSystem::new(),
            fail_reads: Cell::new(false),
            fail_writes: Cell::new(false),
            fail_mkdir: Cell::new(false),
            failure_code: Cell::new(ErrorCode::Io),
        }
    }
}

impl FaultInjector {
    /// An injector that delegates everything until a fault is armed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fails every subsequent file write with `code` (`E_INSUFFICIENT_SPACE` for
    /// a full disk, `E_IO_PERMISSION` for a refused write).
    pub fn fail_writes_with(&self, code: ErrorCode) {
        self.fail_writes.set(true);
        self.failure_code.set(code);
    }

    /// Fails directory creation with `E_IO_PERMISSION`.
    pub fn fail_mkdir(&self) {
        self.fail_mkdir.set(true);
        self.failure_code.set(ErrorCode::IoPermission);
    }

    /// Fails every subsequent file read with `E_IO_PERMISSION`.
    pub fn fail_reads(&self) {
        self.fail_reads.set(true);
    }

    /// Recorded calls, delegated from the inner recorder.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.inner.calls()
    }

    fn failure(&self, path: &SafePath) -> MorphixError {
        MorphixError::new(self.failure_code.get(), "injected filesystem fault")
            .with_context("path", path.as_path().display().to_string())
    }
}

impl FileSystem for FaultInjector {
    fn read_file(&self, path: &SafePath, max_bytes: u64) -> Result<Vec<u8>> {
        if self.fail_reads.get() {
            return Err(
                MorphixError::new(ErrorCode::IoPermission, "injected filesystem fault")
                    .with_context("path", path.as_path().display().to_string()),
            );
        }
        self.inner.read_file(path, max_bytes)
    }

    fn metadata(&self, path: &SafePath) -> Result<FileMetadata> {
        self.inner.metadata(path)
    }

    fn list_dir(&self, path: &SafePath) -> Result<Vec<DirEntry>> {
        self.inner.list_dir(path)
    }

    fn write_file_atomic(&self, path: &SafePath, bytes: &[u8], mode: u32) -> Result<()> {
        if self.fail_writes.get() {
            return Err(self.failure(path));
        }
        self.inner.write_file_atomic(path, bytes, mode)
    }

    fn create_dir_all(&self, path: &SafePath, mode: u32) -> Result<()> {
        if self.fail_mkdir.get() {
            return Err(self.failure(path));
        }
        self.inner.create_dir_all(path, mode)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::RecordingFileSystem;
    use morphix_core::fs::FileSystem;
    use morphix_core::fs::sandbox::SafePath;

    #[test]
    fn records_and_delegates() {
        let directory = tempfile::tempdir().expect("tempdir");
        let file = directory.path().join("note.txt");
        std::fs::write(&file, b"hello").expect("write");
        let path = SafePath::new(&file).expect("safe");

        let fs = RecordingFileSystem::new();
        let bytes = fs.read_file(&path, 1024).expect("read");
        assert_eq!(bytes, b"hello");
        assert_eq!(fs.call_count(), 1);
        assert!(fs.calls()[0].starts_with("read_file:"));
        assert!(fs.calls()[0].ends_with("note.txt"));
    }
}
