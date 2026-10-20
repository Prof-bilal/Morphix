//! Asset structural checks (PACK_SPEC.md §5.5, P2-09).
//!
//! These checks inspect the already-collected [`TreeSnapshot`]; only the `.colors` INI
//! header needs a file read, and that read is capped and never logged.

use std::collections::BTreeSet;
use std::path::Path;

use crate::core::resource::AssetKind;
use crate::errors::ErrorCode;
use crate::fs::tree::{EntryKind, TreeSnapshot};

use super::path::{is_image_path, is_under};
use super::report::ValidationIssue;

/// Files inside an asset directory that would make it look like a nested pack.
const NESTED_PACK_NAMES: [&str; 3] = ["ATTRIBUTION.md", "checksums.sha256", "pack.toml"];

/// Maximum bytes read to verify the `.colors` `[General]` header.
const COLORS_INSPECTION_BYTES: u64 = 1024 * 1024;

/// Checks the structure of a declared asset.
#[must_use]
pub fn check_asset(
    kind: AssetKind,
    root: &Path,
    path: &str,
    tree: &TreeSnapshot,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if kind.is_directory() {
        if tree.entry(path).map(|entry| entry.kind) != Some(EntryKind::Directory) {
            issues.push(
                ValidationIssue::new(
                    ErrorCode::AssetMissing,
                    "the declared asset directory does not exist",
                )
                .at_path(path),
            );
            return issues;
        }
        if directory_file_count(tree, path) == 0 {
            issues.push(
                ValidationIssue::new(ErrorCode::AssetMissing, "the asset directory is empty")
                    .at_path(path),
            );
        }
        for entry in tree.entries.iter().filter(|entry| {
            entry.kind == EntryKind::File
                && entry.relative.starts_with(path)
                && entry.relative.len() > path.len()
        }) {
            let name = entry.relative.rsplit('/').next().unwrap_or("");
            if NESTED_PACK_NAMES.contains(&name) {
                issues.push(
                    ValidationIssue::new(
                        ErrorCode::AssetTypeUnsupported,
                        "an asset directory must not contain a nested pack manifest, \
                         checksum file, or attribution file",
                    )
                    .at_path(&entry.relative),
                );
            }
        }
    } else {
        match tree.entry(path) {
            Some(entry) if entry.kind == EntryKind::File => {}
            _ => {
                issues.push(
                    ValidationIssue::new(
                        ErrorCode::AssetMissing,
                        "the declared asset file does not exist",
                    )
                    .at_path(path),
                );
                return issues;
            }
        }
    }

    match kind {
        AssetKind::ColorScheme => {
            if !path.ends_with(".colors") {
                issues.push(
                    ValidationIssue::new(
                        ErrorCode::AssetTypeUnsupported,
                        "a color scheme must have the .colors extension",
                    )
                    .at_path(path),
                );
            } else if let Some(issue) = inspect_colors_file(root, path) {
                issues.push(issue);
            }
        }
        AssetKind::IconTheme => {
            let index = format!("{path}/index.theme");
            if tree.entry(&index).map(|entry| entry.kind) != Some(EntryKind::File) {
                issues.push(
                    ValidationIssue::new(
                        ErrorCode::AssetTypeUnsupported,
                        "an icon theme must contain index.theme",
                    )
                    .at_path(path),
                );
            }
            for entry in files_under(tree, path) {
                if !has_extension(&entry.relative, &["svg", "png", "theme", "txt"]) {
                    issues.push(
                        ValidationIssue::new(
                            ErrorCode::AssetTypeUnsupported,
                            "icon-theme files must be .svg, .png, .theme, or .txt",
                        )
                        .at_path(&entry.relative),
                    );
                }
            }
        }
        AssetKind::CursorTheme => {
            let cursors = format!("{path}/cursors");
            let has_cursor = tree.entries.iter().any(|entry| {
                entry.kind == EntryKind::File && is_under(&entry.relative, &format!("{cursors}/"))
            });
            if tree.entry(&cursors).map(|entry| entry.kind) != Some(EntryKind::Directory)
                || !has_cursor
            {
                issues.push(
                    ValidationIssue::new(
                        ErrorCode::AssetTypeUnsupported,
                        "a cursor theme must contain a non-empty cursors/ directory",
                    )
                    .at_path(path),
                );
            }
        }
        AssetKind::PlasmaStyle | AssetKind::WindowDecoration => {
            if directory_file_count(tree, path) == 0 {
                issues.push(
                    ValidationIssue::new(ErrorCode::AssetMissing, "the asset directory is empty")
                        .at_path(path),
                );
            }
        }
        AssetKind::Wallpaper => {
            if !is_image_path(path) {
                issues.push(
                    ValidationIssue::new(
                        ErrorCode::AssetTypeUnsupported,
                        "a wallpaper must be a .png, .jpg, .jpeg, or .webp image",
                    )
                    .at_path(path),
                );
            }
        }
    }

    issues
}

/// Validates a preview or icon image path's extension, existence, and size.
///
/// `limit` is 1 MiB for the icon and 5 MiB for screenshots.
#[must_use]
pub fn check_image(
    path: &str,
    tree: &TreeSnapshot,
    limit: u64,
    require_previews_prefix: bool,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if require_previews_prefix && !is_under(path, "previews/") {
        issues.push(
            ValidationIssue::new(
                ErrorCode::AssetTypeUnsupported,
                "the icon and previews must live under previews/",
            )
            .at_path(path),
        );
    }
    if !is_image_path(path) {
        issues.push(
            ValidationIssue::new(
                ErrorCode::AssetTypeUnsupported,
                "only PNG, WebP, and JPEG images are accepted",
            )
            .at_path(path),
        );
    }
    match tree.entry(path) {
        Some(entry) if entry.kind == EntryKind::File => {
            if entry.size == 0 {
                issues.push(
                    ValidationIssue::new(ErrorCode::AssetMissing, "the image is empty")
                        .at_path(path),
                );
            } else if entry.size > limit {
                issues.push(
                    ValidationIssue::new(
                        ErrorCode::LimitExceeded,
                        format!("the image exceeds the {} byte limit", limit),
                    )
                    .at_path(path),
                );
            }
        }
        _ => issues.push(
            ValidationIssue::new(ErrorCode::AssetMissing, "the image file does not exist")
                .at_path(path),
        ),
    }
    issues
}

fn inspect_colors_file(root: &Path, path: &str) -> Option<ValidationIssue> {
    let full = root.join(path);
    let metadata = std::fs::symlink_metadata(&full).ok()?;
    if metadata.len() > COLORS_INSPECTION_BYTES {
        return Some(
            ValidationIssue::new(
                ErrorCode::AssetTypeUnsupported,
                "the color scheme is too large to inspect",
            )
            .at_path(path),
        );
    }
    let bytes = std::fs::read(&full).ok()?;
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Some(
            ValidationIssue::new(
                ErrorCode::AssetTypeUnsupported,
                "the color scheme is not valid UTF-8 INI",
            )
            .at_path(path),
        );
    };
    let has_general = text.lines().any(|line| line.trim() == "[General]");
    if !has_general {
        return Some(
            ValidationIssue::new(
                ErrorCode::AssetTypeUnsupported,
                "a color scheme must contain a [General] section",
            )
            .at_path(path),
        );
    }
    None
}

fn files_under<'a>(
    tree: &'a TreeSnapshot,
    directory: &'a str,
) -> impl Iterator<Item = &'a crate::fs::tree::TreeEntry> {
    tree.entries.iter().filter(move |entry| {
        entry.kind == EntryKind::File && is_under(&entry.relative, &format!("{directory}/"))
    })
}

fn directory_file_count(tree: &TreeSnapshot, directory: &str) -> usize {
    files_under(tree, directory).count()
}

fn has_extension(path: &str, allowed: &[&str]) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| {
            let extension = extension.to_ascii_lowercase();
            allowed.contains(&extension.as_str())
        })
}

/// Files that are referenced by a directory asset, used by the unreferenced-file audit.
#[must_use]
pub fn directory_references(tree: &TreeSnapshot, directory: &str) -> BTreeSet<String> {
    files_under(tree, directory)
        .map(|entry| entry.relative.clone())
        .collect()
}
