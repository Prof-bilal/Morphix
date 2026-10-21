//! Exclusive mutation lock (P4-13, TRANSACTIONS.md invariant 4).
//!
//! **Ownership.** The only mutex between two Morphix processes. The transaction
//! engine holds a [`MutationLock`] for the whole transaction; a second process
//! that tries to mutate gets `E_BUSY`, never a silent queue.
//!
//! **Mechanism.** `flock(LOCK_EX | LOCK_NB)` on a state-directory lock file via
//! `rustix` (the `fs` feature only: no network, no shell, no unsafe in this
//! crate). The lock releases when the guard drops and its fd closes — there is no
//! explicit unlock to forget, and a crashed holder releases the lock with its fd.
//! `flock` locks conflict across separate opens even in the same process, so the
//! contention tests below exercise the real kernel path, not a simulation.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsFd;
use std::path::{Path, PathBuf};

use crate::errors::{ErrorCode, MorphixError, Result};

use super::sandbox::SafePath;

/// Private mode for the lock file.
const LOCK_FILE_MODE: u32 = 0o600;

/// An held exclusive mutation lock. Dropping releases it.
#[derive(Debug)]
pub struct MutationLock {
    /// The open lock file: its fd carries the `flock`, so the field must stay
    /// alive as long as the guard does. Never read or written.
    _file: fs::File,
    /// Where the lock lives, for diagnostics.
    path: PathBuf,
}

impl MutationLock {
    /// Acquires the exclusive lock without blocking.
    ///
    /// The lock file is created with a private mode when missing. An already-held
    /// lock fails with `E_BUSY` instead of waiting: a second mutation must surface
    /// immediately, not stall behind the first.
    ///
    /// # Errors
    ///
    /// Returns `E_BUSY` when another process (or thread) holds the lock, and a
    /// typed I/O error when the lock file cannot be opened.
    pub fn acquire(lock_path: &SafePath) -> Result<Self> {
        use super::sandbox::io_error;

        // The fd only carries the flock; the file is never written or truncated.
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path.as_path())
            .map_err(|error| io_error(lock_path.as_path(), &error))?;
        fs::set_permissions(
            lock_path.as_path(),
            fs::Permissions::from_mode(LOCK_FILE_MODE),
        )
        .map_err(|error| io_error(lock_path.as_path(), &error))?;
        rustix::fs::flock(
            file.as_fd(),
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        )
        .map_err(|error| {
            if error == rustix::io::Errno::WOULDBLOCK {
                MorphixError::new(ErrorCode::Busy, "another mutation holds the lock")
                    .with_context("path", lock_path.as_path().display().to_string())
            } else {
                MorphixError::new(ErrorCode::Io, "a filesystem operation failed")
                    .with_context("path", lock_path.as_path().display().to_string())
                    .with_context("io_kind", format!("errno({})", error.raw_os_error()))
            }
        })?;
        Ok(Self {
            _file: file,
            path: lock_path.as_path().to_path_buf(),
        })
    }

    /// Where the lock lives.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::{Arc, Barrier};

    use super::MutationLock;
    use crate::errors::ErrorCode;
    use crate::fs::sandbox::SafePath;

    fn lock_path(directory: &tempfile::TempDir) -> SafePath {
        SafePath::new(directory.path().join("mutation.lock")).expect("safe path")
    }

    #[test]
    fn second_acquire_is_busy_until_release() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = lock_path(&directory);

        let first = MutationLock::acquire(&path).expect("first acquire");
        assert_eq!(
            MutationLock::acquire(&path).expect_err("contended").code,
            ErrorCode::Busy
        );
        drop(first);
        MutationLock::acquire(&path).expect("acquire after release");
    }

    #[test]
    fn contention_across_threads_is_busy() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = lock_path(&directory);
        let barrier = Arc::new(Barrier::new(2));

        let held = MutationLock::acquire(&path).expect("main acquire");
        let worker_barrier = Arc::clone(&barrier);
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            let error = MutationLock::acquire(&path).expect_err("worker must be busy");
            assert_eq!(error.code, ErrorCode::Busy);
        });
        barrier.wait();
        worker.join().expect("worker");
        drop(held);
    }

    #[test]
    fn lock_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("tempdir");
        let _held = MutationLock::acquire(&lock_path(&directory)).expect("acquire");
        let mode = std::fs::metadata(directory.path().join("mutation.lock"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
