//! Catalog assembly and pack detail DTOs (P2-12, AC-05/AC-06/AC-08).
//!
//! Catalog load is read-only: it re-validates each pack directory and never writes a row,
//! a cache file, or a log of pack contents. Invalid packs are surfaced with their issues,
//! never silently dropped (ARCHITECTURE.md §7.1). DTOs are presentation contracts, separate
//! from the typed manifest.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::ids::InstalledName;
use crate::core::resource::{AssetKind, IntegritySource, PackSource, PackStatus};
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::fs::tree::EntryKind;

use super::manifest::Manifest;
use super::report::{DeclaredChange, ValidationIssue, ValidationOutcome};
use super::validator::validate_directory;

/// Maximum attribution bytes shown in the detail view.
const ATTRIBUTION_LIMIT: u64 = 64 * 1024;

/// A directory the catalog should try to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Pack root (already resolved by the service).
    pub root: PathBuf,
    /// Bundled or local.
    pub source: PackSource,
    /// Whether the pack is currently applied.
    pub applied: bool,
    /// Stable fallback when the manifest cannot be parsed.
    pub fallback_id: String,
}

/// Catalog card data (AC-05).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    /// Pack ID, or the directory name when the manifest is unparseable.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Strict SemVer version, when parsed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// One-line summary.
    pub summary: String,
    /// Bundled or local.
    pub source: PackSource,
    /// Valid or invalid.
    pub status: PackStatus,
    /// Plasma range expression, when parsed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    /// Requested capability IDs.
    pub capabilities: Vec<String>,
    /// Icon path, relative to the pack root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Whether the pack is currently applied.
    pub applied: bool,
    /// Declared checksums or an install-time hash.
    pub integrity_source: IntegritySource,
    /// Number of validation issues (the detail carries the full list).
    pub issue_count: usize,
}

/// One asset line in the detail view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetView {
    /// Asset kind ID (`color_scheme`, …).
    pub kind: String,
    /// Manifest key.
    pub key: String,
    /// Pack-relative source path.
    pub path: String,
    /// Engine-derived on-disk name.
    pub installed_name: String,
    /// Whether the asset is a directory tree.
    pub is_directory: bool,
    /// Recursive size in bytes.
    pub bytes: u64,
}

/// One preview image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    /// Caption shown below the image.
    pub caption: String,
    /// Pack-relative path from the manifest.
    pub image: String,
    /// Absolute path inside the validated pack store.
    pub absolute_path: String,
}

/// One dependency line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyView {
    /// Required pack ID.
    pub id: String,
    /// SemVer range.
    pub version: String,
}

/// Full pack detail (AC-06, AC-08).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackDetail {
    /// Card data.
    pub summary: PackSummary,
    /// Markdown description, rendered as text by the UI.
    pub description: String,
    /// Author names.
    pub authors: Vec<String>,
    /// SPDX expression.
    pub license: String,
    /// Display-only homepage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Attribution document text, rendered as plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    /// Every declared asset.
    pub assets: Vec<AssetView>,
    /// Every declared change, by capability and target kind.
    pub declared_changes: Vec<DeclaredChange>,
    /// Preview images.
    pub previews: Vec<PreviewView>,
    /// Declared dependencies.
    pub dependencies: Vec<DependencyView>,
    /// Full validation issue list (empty when valid).
    pub issues: Vec<ValidationIssue>,
    /// Canonical manifest hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_hash: Option<String>,
    /// Deterministic tree hash.
    pub content_hash: String,
}

/// Loads the catalog cards for the given candidates.
///
/// Duplicate IDs are skipped after the first candidate, so the service orders installed
/// packs before bundled directories. An invalid pack still produces a card.
///
/// # Errors
///
/// Returns a typed I/O error only when a candidate tree cannot be inspected.
pub fn list(candidates: &[Candidate]) -> Result<Vec<PackSummary>> {
    let mut seen = BTreeSet::new();
    let mut summaries = Vec::new();
    for candidate in candidates {
        let outcome = load_outcome(candidate);
        let summary = summarize(candidate, &outcome);
        if seen.insert(summary.id.clone()) {
            summaries.push(summary);
        }
    }
    summaries.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(summaries)
}

/// Loads one pack detail, or `None` when no candidate has that ID.
///
/// # Errors
///
/// Returns a typed I/O error only if the caller-supplied candidate list itself cannot be
/// read; an unreadable pack root becomes an invalid detail instead.
pub fn detail(candidates: &[Candidate], pack_id: &str) -> Result<Option<PackDetail>> {
    for candidate in candidates {
        let outcome = load_outcome(candidate);
        let summary = summarize(candidate, &outcome);
        if summary.id == pack_id {
            return Ok(Some(build_detail(candidate, outcome, summary)));
        }
    }
    Ok(None)
}

/// Runs validation, turning an unreadable root into a single invalid outcome so the
/// catalog can still show the pack with a specific error (AC-05/AC-06).
fn load_outcome(candidate: &Candidate) -> ValidationOutcome {
    match validate_directory(&candidate.root) {
        Ok(outcome) => outcome,
        Err(error) => {
            let issue = ValidationIssue::new(error.code, error.message);
            ValidationOutcome {
                manifest: None,
                manifest_hash: None,
                content_hash: String::new(),
                tree: crate::fs::tree::TreeSnapshot::default(),
                manifest_bytes: 0,
                issues: vec![issue],
                declared_changes: Vec::new(),
                integrity_source: IntegritySource::GeneratedLocal,
            }
        }
    }
}

/// Scans a bundled-packs directory for candidate roots.
///
/// Missing roots are empty, not an error. Hidden entries and symlinks are skipped. The
/// result is sorted by directory name for deterministic ordering.
///
/// # Errors
///
/// Returns a typed I/O error when the directory exists but cannot be read.
pub fn scan_directory(root: &Path) -> Result<Vec<PathBuf>> {
    let metadata = match std::fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(crate::fs::sandbox::io_error(root, &error)),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(MorphixError::new(
            ErrorCode::FileTypeRejected,
            "the bundled pack root is not a directory",
        )
        .with_context("path", root.display().to_string()));
    }

    let mut directories = Vec::new();
    let entries =
        std::fs::read_dir(root).map_err(|error| crate::fs::sandbox::io_error(root, &error))?;
    for entry in entries {
        let entry = entry.map_err(|error| crate::fs::sandbox::io_error(root, &error))?;
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        directories.push(path);
    }
    directories.sort();
    Ok(directories)
}

/// Builds a summary from one validation outcome.
#[must_use]
pub fn summarize(candidate: &Candidate, outcome: &ValidationOutcome) -> PackSummary {
    let manifest = outcome.manifest.as_ref();
    let id = manifest
        .map(|manifest| manifest.pack.id.to_string())
        .unwrap_or_else(|| candidate.fallback_id.clone());
    let status = if outcome.is_valid() {
        PackStatus::Valid
    } else {
        PackStatus::Invalid
    };
    PackSummary {
        id,
        name: manifest
            .map(|manifest| manifest.pack.name.clone())
            .unwrap_or_else(|| candidate.fallback_id.clone()),
        version: manifest.map(|manifest| manifest.pack.version.to_string()),
        summary: manifest
            .map(|manifest| manifest.pack.summary.clone())
            .unwrap_or_default(),
        source: candidate.source,
        status,
        compatibility: manifest.map(|manifest| manifest.compatibility.plasma.clone()),
        capabilities: manifest
            .map(|manifest| manifest.capabilities.requires.clone())
            .unwrap_or_default(),
        icon: manifest.map(|manifest| manifest.pack.icon.clone()),
        applied: candidate.applied,
        integrity_source: outcome.integrity_source,
        issue_count: outcome.issues.len(),
    }
}

fn build_detail(
    candidate: &Candidate,
    outcome: ValidationOutcome,
    summary: PackSummary,
) -> PackDetail {
    let manifest = outcome.manifest.as_ref();
    PackDetail {
        summary,
        description: manifest
            .map(|manifest| manifest.pack.description.clone())
            .unwrap_or_default(),
        authors: manifest
            .map(|manifest| manifest.pack.authors.clone())
            .unwrap_or_default(),
        license: manifest
            .map(|manifest| manifest.pack.license.clone())
            .unwrap_or_default(),
        homepage: manifest.and_then(|manifest| manifest.pack.homepage.clone()),
        attribution: read_attribution(&candidate.root),
        assets: asset_views(manifest, &outcome),
        declared_changes: outcome.declared_changes.clone(),
        previews: preview_views(manifest, &candidate.root),
        dependencies: manifest
            .and_then(|manifest| manifest.dependencies.as_ref())
            .map(|dependencies| {
                dependencies
                    .packs
                    .iter()
                    .map(|dependency| DependencyView {
                        id: dependency.id.clone(),
                        version: dependency.version.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        issues: outcome.issues,
        manifest_hash: outcome.manifest_hash,
        content_hash: outcome.content_hash,
    }
}

fn asset_views(manifest: Option<&Manifest>, outcome: &ValidationOutcome) -> Vec<AssetView> {
    let Some(manifest) = manifest else {
        return Vec::new();
    };
    let mut views = Vec::new();
    for (kind, key, path) in manifest.assets_iter() {
        let installed_name = InstalledName::derive(&manifest.pack.id, key)
            .map(|name| name.to_string())
            .unwrap_or_default();
        views.push(AssetView {
            kind: kind.as_str().to_owned(),
            key: key.to_string(),
            path: path.to_owned(),
            installed_name,
            is_directory: kind.is_directory(),
            bytes: size_under(outcome, path),
        });
    }
    views
}

fn preview_views(manifest: Option<&Manifest>, root: &Path) -> Vec<PreviewView> {
    manifest
        .map(|manifest| {
            manifest
                .previews
                .iter()
                .map(|preview| PreviewView {
                    caption: preview.caption.clone(),
                    image: preview.image.clone(),
                    absolute_path: root.join(&preview.image).display().to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn size_under(outcome: &ValidationOutcome, path: &str) -> u64 {
    outcome
        .tree
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .filter(|entry| entry.relative == path || entry.relative.starts_with(&format!("{path}/")))
        .map(|entry| entry.size)
        .sum()
}

fn read_attribution(root: &Path) -> Option<String> {
    let path = root.join("ATTRIBUTION.md");
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > ATTRIBUTION_LIMIT {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Maps an `AssetKind` to its stable identifier, for callers building DTOs.
#[must_use]
pub fn asset_kind_id(kind: AssetKind) -> &'static str {
    kind.as_str()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::path::Path;

    use super::{Candidate, detail, list, scan_directory};
    use crate::core::resource::PackSource;

    fn write(path: &Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, contents).expect("write");
    }

    fn valid_pack(root: &Path, id: &str, name: &str) {
        write(
            &root.join("pack.toml"),
            format!(
                r#"
schema_version = 1

[pack]
id = "{id}"
name = "{name}"
version = "1.2.3"
summary = "A test pack."
description = "A test pack used by the catalog tests."
authors = ["Tester"]
license = "MIT"
icon = "previews/icon.png"

[compatibility]
desktop = "kde"
plasma = ">=6.0, <7.0"

[capabilities]
requires = ["appearance.wallpaper"]

[assets]

[appearance]
wallpaper = "wall"

[assets.wallpapers]
wall = "wallpapers/a.png"

[[previews]]
image = "previews/icon.png"
caption = "Preview"
"#
            )
            .as_bytes(),
        );
        write(&root.join("README.md"), b"# Pack\n");
        write(&root.join("ATTRIBUTION.md"), b"Original assets.\n");
        write(&root.join("previews/icon.png"), b"\x89PNG\r\n\x1a\n");
        write(&root.join("wallpapers/a.png"), b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn valid_and_invalid_packs_both_produce_cards() {
        let temp = tempfile::tempdir().expect("temp");
        let good = temp.path().join("good");
        valid_pack(&good, "dev.example.good", "Good");
        let bad = temp.path().join("bad");
        std::fs::create_dir_all(&bad).expect("bad dir");
        write(&bad.join("pack.toml"), b"schema_version = 9\n");

        let candidates = vec![
            Candidate {
                root: good,
                source: PackSource::Bundled,
                applied: false,
                fallback_id: "good".to_owned(),
            },
            Candidate {
                root: bad,
                source: PackSource::Local,
                applied: false,
                fallback_id: "bad".to_owned(),
            },
        ];
        let cards = list(&candidates).expect("list");
        assert_eq!(cards.len(), 2);
        assert_eq!(
            cards
                .iter()
                .find(|card| card.id == "dev.example.good")
                .expect("good card")
                .status,
            crate::core::resource::PackStatus::Valid
        );
        assert_eq!(
            cards
                .iter()
                .find(|card| card.id == "bad")
                .expect("bad card")
                .status,
            crate::core::resource::PackStatus::Invalid
        );
    }

    #[test]
    fn detail_lists_assets_and_declared_changes() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("pack");
        valid_pack(&root, "dev.example.detail", "Detail");
        let candidates = vec![Candidate {
            root,
            source: PackSource::Local,
            applied: true,
            fallback_id: "detail".to_owned(),
        }];
        let detail = detail(&candidates, "dev.example.detail")
            .expect("detail")
            .expect("found");
        assert_eq!(detail.assets.len(), 1);
        assert_eq!(detail.assets[0].installed_name, "detail-wall");
        assert!(
            detail
                .declared_changes
                .iter()
                .any(|change| change.kind == "set_wallpaper")
        );
        assert_eq!(detail.attribution.as_deref(), Some("Original assets.\n"));
        assert!(detail.summary.applied);
    }

    #[test]
    fn missing_scan_root_is_empty() {
        let temp = tempfile::tempdir().expect("temp");
        assert!(
            scan_directory(&temp.path().join("missing"))
                .expect("scan")
                .is_empty()
        );
    }
}
