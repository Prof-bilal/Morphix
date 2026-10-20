//! Pack-relative path rules (PACK_SPEC.md §2, P2-05).
//!
//! These checks run before any path is joined onto a root. A path is a single `/`-separated
//! UTF-8 string: no absolute paths, drive prefixes, backslashes, empty components, `.`,
//! `..`, control characters, or NUL. Depth is capped at eight and the whole path at 255
//! characters. Names are capped at 255 bytes.
//!
//! Symlink, hardlink, file-type, and executable-bit checks are performed on the tree
//! snapshot in [`super::validator`] and [`crate::fs::tree`]; this module is pure.

use std::path::{Path, PathBuf};

use crate::errors::{ErrorCode, MorphixError, Result};

/// Maximum path depth (root children have depth 1).
pub const MAX_PATH_DEPTH: usize = 8;

/// Maximum path length in characters.
pub const MAX_PATH_CHARS: usize = 255;

/// Maximum single file-name length in bytes.
pub const MAX_NAME_BYTES: usize = 255;

/// Prefixes an asset or preview path may use (PACK_SPEC.md §2).
pub const ASSET_PREFIXES: [&str; 6] = [
    "cursors/",
    "icons/",
    "plasma/",
    "previews/",
    "themes/",
    "wallpapers/",
];

/// Root files that are never asset paths.
pub const ROOT_FILES: [&str; 4] = [
    "ATTRIBUTION.md",
    "README.md",
    "checksums.sha256",
    "pack.toml",
];

/// Validates a pack-relative path and returns it unchanged.
///
/// # Errors
///
/// `E_PATH_TRAVERSAL` for a `..` component, `E_INVALID_PATH` for every other grammar
/// violation.
pub fn validate_relative(path: &str) -> Result<&str> {
    if path.is_empty() {
        return Err(invalid_path(path, "the path is empty"));
    }
    if path.len() > MAX_PATH_CHARS {
        return Err(invalid_path(path, "the path is longer than 255 characters"));
    }
    if path.as_bytes().contains(&0) {
        return Err(invalid_path(path, "the path contains a NUL byte"));
    }
    if path.chars().any(char::is_control) {
        return Err(invalid_path(path, "the path contains a control character"));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(invalid_path(path, "the path is absolute"));
    }
    if path.contains('\\') {
        return Err(invalid_path(
            path,
            "the path uses a backslash instead of a forward slash",
        ));
    }
    if path.ends_with('/') {
        return Err(invalid_path(path, "the path ends with a separator"));
    }

    let components: Vec<&str> = path.split('/').collect();
    if components.len() > MAX_PATH_DEPTH {
        return Err(invalid_path(path, "the path is deeper than eight levels"));
    }
    for component in components {
        match component {
            "" => return Err(invalid_path(path, "the path has an empty component")),
            "." => return Err(invalid_path(path, "the path has a dot component")),
            ".." => {
                return Err(MorphixError::new(
                    ErrorCode::PathTraversal,
                    "the path contains a parent-directory component",
                )
                .with_context("path", path));
            }
            _ => {}
        }
        if component.len() > MAX_NAME_BYTES {
            return Err(invalid_path(
                path,
                "a path component is longer than 255 bytes",
            ));
        }
    }

    Ok(path)
}

/// Validates an asset or preview path: a safe relative path under an allowed prefix.
///
/// # Errors
///
/// As [`validate_relative`], plus `E_INVALID_PATH` when the path is not under one of
/// `themes/`, `icons/`, `cursors/`, `plasma/`, `wallpapers/`, or `previews/`.
pub fn validate_asset_path(path: &str) -> Result<&str> {
    validate_relative(path)?;
    if !ASSET_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return Err(invalid_path(
            path,
            "an asset path must start with themes/, icons/, cursors/, plasma/, wallpapers/, or previews/",
        ));
    }
    Ok(path)
}

/// Whether a validated path sits under a prefix such as `previews/`.
#[must_use]
pub fn is_under(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix)
        .is_some_and(|rest| !rest.is_empty())
}

/// Whether a path is a root-level manifest/document file.
#[must_use]
pub fn is_root_file(path: &str) -> bool {
    ROOT_FILES.contains(&path)
}

/// Joins a validated relative path onto a root without allowing escape.
///
/// The caller still has to check symlinks with
/// [`crate::fs::sandbox::SafePath::require_no_symlink_ancestors`] before writing.
///
/// # Errors
///
/// Returns `E_PATH_TRAVERSAL` if joining would escape the root.
pub fn join_under(root: &Path, relative: &str) -> Result<PathBuf> {
    validate_relative(relative)?;
    let joined = root.join(relative);
    if !joined.starts_with(root) {
        return Err(MorphixError::new(
            ErrorCode::PathTraversal,
            "the joined path escapes the pack root",
        )
        .with_context("root", root.display().to_string())
        .with_context("path", relative));
    }
    Ok(joined)
}

/// Whether an extension is one of the accepted raster image types.
#[must_use]
pub fn is_image_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp"
            )
        })
}

fn invalid_path(path: &str, reason: &'static str) -> MorphixError {
    MorphixError::new(ErrorCode::InvalidPath, "the pack path is not safe")
        .with_context("path", path)
        .with_context("reason", reason)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{is_image_path, validate_asset_path, validate_relative};
    use crate::errors::ErrorCode;

    #[test]
    fn relative_asset_paths_are_accepted() {
        for path in [
            "themes/color-schemes/Dark.colors",
            "icons/Theme/index.theme",
            "previews/desktop-light.webp",
            "wallpapers/a/b/c/d/e/f.webp",
        ] {
            assert_eq!(validate_relative(path), Ok(path));
        }
    }

    #[test]
    fn traversal_and_absolute_paths_are_refused() {
        for path in ["/etc/passwd", "../pack.toml", "themes/../../x", "a//b"] {
            assert!(validate_asset_path(path).is_err(), "{path}");
        }
        assert_eq!(
            validate_asset_path("wallpapers/../secrets")
                .expect_err("traversal")
                .code,
            ErrorCode::PathTraversal
        );
    }

    #[test]
    fn depth_length_and_control_limits_are_enforced() {
        let too_deep = "themes/a/b/c/d/e/f/g/h";
        assert!(validate_relative(too_deep).is_err());

        let long_name = format!("wallpapers/{}", "a".repeat(256));
        assert!(validate_relative(&long_name).is_err());

        assert!(validate_relative("wallpapers/nul\u{0}.png").is_err());
        assert!(validate_relative("wallpapers/new\nline.png").is_err());
        assert!(validate_relative("wallpapers/back\\slash.png").is_err());
    }

    #[test]
    fn only_allowed_prefixes_and_image_extensions_pass() {
        assert!(validate_asset_path("README.md").is_err());
        assert!(validate_asset_path("other/file.png").is_err());
        assert!(validate_asset_path("wallpapers/a.png").is_ok());
        assert!(is_image_path("wallpapers/a.PNG"));
        assert!(!is_image_path("wallpapers/a.gif"));
        assert!(!is_image_path("wallpapers/a"));
    }
}
