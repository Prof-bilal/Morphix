//! Absolute-path sandbox primitives (Phase 2 read-only subset; completed in P4-03).
//!
//! **Ownership.** `SafePath` is the only way an operation may name a host path. It rejects
//! relative paths, `..`, and NUL bytes before any syscall, and can re-check the existing
//! ancestor chain for symlinks immediately before a write (SECURITY.md §8, T-18).
//!
//! This module does not decide *which* roots are writable: the fixed target map does that
//! (Phase 3/6). It only proves a path is syntactically contained where the caller says.

use std::path::{Component, Path, PathBuf};

use crate::errors::{ErrorCode, MorphixError, Result};

/// An absolute path with no `..` or NUL components.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SafePath {
    path: PathBuf,
}

impl SafePath {
    /// Wraps an absolute, already-normalized path.
    ///
    /// # Errors
    ///
    /// Returns `E_INVALID_PATH` for relative paths, `E_PATH_TRAVERSAL` for a `..`
    /// component, and `E_INVALID_PATH` for a NUL byte.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(invalid_path(path.as_path(), "the path is not absolute"));
        }
        if path.as_os_str().as_encoded_bytes().contains(&0) {
            return Err(invalid_path(path.as_path(), "the path contains a NUL byte"));
        }
        // `Path::components` normalizes `.` away, so inspect the raw bytes as well to keep
        // the promise that a dot component never reaches a syscall.
        for raw in path
            .as_os_str()
            .as_encoded_bytes()
            .split(|byte| *byte == b'/')
        {
            match raw {
                b"." => {
                    return Err(invalid_path(
                        path.as_path(),
                        "the path contains a current-directory component",
                    ));
                }
                b".." => {
                    return Err(MorphixError::new(
                        ErrorCode::PathTraversal,
                        "the path contains a parent-directory component",
                    )
                    .with_context("path", path.display().to_string()));
                }
                _ => {}
            }
        }
        for component in path.components() {
            match component {
                Component::ParentDir => {
                    return Err(MorphixError::new(
                        ErrorCode::PathTraversal,
                        "the path contains a parent-directory component",
                    )
                    .with_context("path", path.display().to_string()));
                }
                Component::CurDir => {
                    return Err(invalid_path(
                        path.as_path(),
                        "the path contains a current-directory component",
                    ));
                }
                Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {}
            }
        }
        Ok(Self { path })
    }

    /// Joins a validated relative path onto this root.
    ///
    /// # Errors
    ///
    /// The relative value must be non-empty, must not be absolute, and must not contain
    /// `..`, `.`, or NUL. The result is checked with [`Path::starts_with`] as defense in
    /// depth.
    pub fn join_relative(&self, relative: &str) -> Result<Self> {
        validate_relative(relative)?;
        let joined = self.path.join(relative);
        if !joined.starts_with(&self.path) {
            return Err(MorphixError::new(
                ErrorCode::PathTraversal,
                "the joined path escapes its root",
            )
            .with_context("root", self.path.display().to_string())
            .with_context("relative", relative));
        }
        Self::new(joined)
    }

    /// The underlying path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.path
    }

    /// Whether another safe path is inside this one.
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        other.path.starts_with(&self.path)
    }

    /// The parent directory, when this path is not the filesystem root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        self.path.parent().and_then(|parent| Self::new(parent).ok())
    }

    /// The final component, when this path is not the filesystem root.
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.path.file_name().and_then(|name| name.to_str())
    }

    /// The path of `self` relative to `root`, for manifest entries and logs.
    ///
    /// # Errors
    ///
    /// Returns `E_PATH_TRAVERSAL` when `self` is not inside `root`.
    pub fn relative_to(&self, root: &Self) -> Result<std::path::PathBuf> {
        self.path
            .strip_prefix(&root.path)
            .map(std::path::Path::to_path_buf)
            .map_err(|_| {
                MorphixError::new(ErrorCode::PathTraversal, "the path is outside its root")
                    .with_context("root", root.path.display().to_string())
                    .with_context("path", self.path.display().to_string())
            })
    }

    /// Rejects an existing symlink in the ancestor chain, including the final component.
    ///
    /// Missing components are allowed: a caller creating a new file must still be able to
    /// create its parent later, and each parent is checked when it exists.
    ///
    /// # Errors
    ///
    /// Returns `E_SYMLINK_TARGET` when any existing component is a symbolic link, and a
    /// typed I/O error when a component cannot be inspected.
    pub fn require_no_symlink_ancestors(&self) -> Result<()> {
        let mut current = PathBuf::new();
        for component in self.path.components() {
            current.push(component.as_os_str());
            match std::fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(MorphixError::new(
                        ErrorCode::SymlinkTarget,
                        "Morphix refuses to write through a symbolic link",
                    )
                    .with_context("path", current.display().to_string()));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => {
                    return Err(io_error(&current, &error));
                }
            }
        }
        Ok(())
    }
}

/// Validates a pack-relative path used by generic filesystem joins.
fn validate_relative(relative: &str) -> Result<()> {
    if relative.is_empty() {
        return Err(invalid_relative(relative, "the path is empty"));
    }
    if relative.as_bytes().contains(&0) {
        return Err(invalid_relative(relative, "the path contains a NUL byte"));
    }
    if relative.starts_with('/') || relative.starts_with('\\') {
        return Err(invalid_relative(relative, "the path is absolute"));
    }
    for component in relative.split('/') {
        match component {
            "" => {
                return Err(invalid_relative(
                    relative,
                    "the path has an empty component",
                ));
            }
            "." => return Err(invalid_relative(relative, "the path has a dot component")),
            ".." => {
                return Err(MorphixError::new(
                    ErrorCode::PathTraversal,
                    "the path contains a parent-directory component",
                )
                .with_context("path", relative));
            }
            _ => {}
        }
    }
    Ok(())
}

fn invalid_path(path: &Path, reason: &'static str) -> MorphixError {
    MorphixError::new(ErrorCode::InvalidPath, "the path is not safe")
        .with_context("path", path.display().to_string())
        .with_context("reason", reason)
}

fn invalid_relative(relative: &str, reason: &'static str) -> MorphixError {
    MorphixError::new(ErrorCode::InvalidPath, "the relative path is not safe")
        .with_context("path", relative)
        .with_context("reason", reason)
}

/// Maps an I/O failure to the stable permission or generic I/O code.
#[must_use]
pub fn io_error(path: &Path, error: &std::io::Error) -> MorphixError {
    let code = if error.kind() == std::io::ErrorKind::PermissionDenied {
        ErrorCode::IoPermission
    } else {
        ErrorCode::Io
    };
    MorphixError::new(code, "a filesystem operation failed")
        .with_context("path", path.display().to_string())
        .with_context("io_kind", format!("{:?}", error.kind()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::path::Path;

    use super::SafePath;
    use crate::errors::ErrorCode;

    #[test]
    fn absolute_paths_without_parent_components_are_accepted() {
        let path = SafePath::new("/home/tester/.config/kdeglobals").expect("safe path");
        assert_eq!(path.as_path(), Path::new("/home/tester/.config/kdeglobals"));
    }

    #[test]
    fn relative_and_traversal_paths_are_refused_before_io() {
        assert_eq!(
            SafePath::new("relative/file").expect_err("relative").code,
            ErrorCode::InvalidPath
        );
        assert_eq!(
            SafePath::new("/home/../etc/passwd")
                .expect_err("traversal")
                .code,
            ErrorCode::PathTraversal
        );
        assert_eq!(
            SafePath::new("/home/./file").expect_err("dot").code,
            ErrorCode::InvalidPath
        );
    }

    #[test]
    fn nuls_are_refused() {
        let error = SafePath::new("/home/tester/bad\0name").expect_err("NUL");
        assert_eq!(error.code, ErrorCode::InvalidPath);
    }

    #[test]
    fn join_relative_stays_inside_the_root() {
        let root = SafePath::new("/home/tester/.local/share/morphix").expect("root");
        let joined = root
            .join_relative("packs/dev.morphix.cupertino")
            .expect("joined");
        assert_eq!(
            joined.as_path(),
            Path::new("/home/tester/.local/share/morphix/packs/dev.morphix.cupertino")
        );
        assert!(root.contains(&joined));

        for relative in [
            "",
            "/etc/passwd",
            "../escape",
            "a/../../escape",
            "./a",
            "a//b",
        ] {
            assert!(
                root.join_relative(relative).is_err(),
                "relative path must be refused: {relative:?}"
            );
        }
    }

    #[test]
    fn existing_symlink_ancestors_are_rejected() {
        let directory = tempfile::tempdir().expect("tempdir");
        let real = directory.path().join("real");
        let link = directory.path().join("link");
        std::fs::create_dir(&real).expect("create real");
        std::os::unix::fs::symlink(&real, &link).expect("create symlink");

        let target = SafePath::new(link.join("file")).expect("absolute");
        assert_eq!(
            target
                .require_no_symlink_ancestors()
                .expect_err("symlink")
                .code,
            ErrorCode::SymlinkTarget
        );

        let missing = SafePath::new(real.join("missing/file")).expect("absolute");
        assert!(missing.require_no_symlink_ancestors().is_ok());
    }

    #[test]
    fn final_component_symlinks_are_rejected() {
        let directory = tempfile::tempdir().expect("tempdir");
        let real = directory.path().join("real.txt");
        let link = directory.path().join("link.txt");
        std::fs::write(&real, b"real").expect("write real");
        std::os::unix::fs::symlink(&real, &link).expect("create symlink");

        let target = SafePath::new(&link).expect("absolute");
        assert_eq!(
            target
                .require_no_symlink_ancestors()
                .expect_err("final symlink")
                .code,
            ErrorCode::SymlinkTarget
        );
    }

    #[test]
    fn containment_helpers_agree_with_prefix_checks() {
        let root = SafePath::new("/home/tester/.config").expect("root");
        let inside = SafePath::new("/home/tester/.config/kdeglobals").expect("inside");
        let outside = SafePath::new("/home/tester/.local/share").expect("outside");

        assert!(root.contains(&inside));
        assert!(!root.contains(&outside));
        assert_eq!(
            inside.relative_to(&root).expect("relative").as_os_str(),
            "kdeglobals"
        );
        assert_eq!(
            inside.relative_to(&outside).expect_err("outside root").code,
            ErrorCode::PathTraversal
        );
        assert_eq!(
            inside.parent().expect("parent").as_path(),
            std::path::Path::new("/home/tester/.config")
        );
        assert_eq!(inside.file_name(), Some("kdeglobals"));
        assert!(SafePath::new("/").expect("fs root").parent().is_none());
    }

    /// Model-based containment test: a deterministic generator builds paths from an
    /// adversarial alphabet, and an independent component-walk oracle must agree with
    /// `SafePath::new` on every input. No external property-test crate: the oracle is
    /// a second implementation of the rule, not a copy of the code under test.
    #[test]
    fn generated_paths_match_the_component_oracle() {
        let components = [
            "home", "tester", ".config", ".", "..", "", "a b", "..hidden", "x..y",
        ];
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        for _ in 0..2000 {
            let absolute = next().is_multiple_of(2);
            let width = 1 + next() % 5;
            let mut raw = String::new();
            if absolute {
                raw.push('/');
            }
            for index in 0..width {
                if index > 0 {
                    raw.push('/');
                }
                raw.push_str(components[(next() as usize) % components.len()]);
            }
            if next().is_multiple_of(7) {
                raw.push('\0');
            }

            assert_eq!(
                SafePath::new(raw.clone()).is_ok(),
                oracle_accepts(&raw),
                "oracle disagreement on {raw:?}"
            );
        }
    }

    /// Accepts exactly the paths `SafePath::new` must accept: absolute, NUL-free, and
    /// with no `.` or `..` raw component. Written against the rule, not the code.
    fn oracle_accepts(raw: &str) -> bool {
        if !raw.starts_with('/') || raw.as_bytes().contains(&0) {
            return false;
        }
        !raw.split('/').any(|part| part == "." || part == "..")
    }
}
