//! Shared pack fixtures for the validation and install integration tests (P2-14).
//!
//! `PackBuilder` deliberately writes real directories instead of fixtures checked into
//! the repository: most validation codes depend on the tree (symlinks, executable bits,
//! missing files, checksums), and a builder keeps each fixture next to the rule it
//! exercises. Every mutation is explicit, so a fixture cannot accidentally become valid
//! when the base manifest changes.
//!
//! The base pack uses only documented schema-v1 forms (PACK_SPEC.md §4–§6.1).

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use morphix_core::fs::tree::{self, EntryKind};
use sha2::{Digest, Sha256};

/// A valid schema-v1 pack with one wallpaper asset.
pub const BASE_MANIFEST: &str = r#"schema_version = 1

[pack]
id = "dev.example.fixture"
name = "Fixture Pack"
version = "1.0.0"
summary = "A fixture pack used by the validation tests."
description = "A fixture pack used by the validation tests. It is intentionally small."
authors = ["Morphix Tests"]
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
wall = "wallpapers/wall.png"

[[previews]]
image = "previews/icon.png"
caption = "Fixture preview"
"#;

const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Builds a pack tree on disk, starting from a valid pack.
#[derive(Debug, Clone)]
pub struct PackBuilder {
    root: PathBuf,
    manifest: String,
    raw_manifest: Option<Vec<u8>>,
    files: BTreeMap<String, Vec<u8>>,
    removed: BTreeSet<String>,
    modes: BTreeMap<String, u32>,
    symlinks: BTreeMap<String, PathBuf>,
}

impl PackBuilder {
    /// Starts a valid pack rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let mut files = BTreeMap::new();
        files.insert("README.md".to_owned(), b"# Fixture pack\n".to_vec());
        files.insert(
            "ATTRIBUTION.md".to_owned(),
            b"Fixture assets, original work.\n".to_vec(),
        );
        files.insert("previews/icon.png".to_owned(), PNG_HEADER.to_vec());
        files.insert("wallpapers/wall.png".to_owned(), PNG_HEADER.to_vec());
        Self {
            root: root.into(),
            manifest: BASE_MANIFEST.to_owned(),
            raw_manifest: None,
            files,
            removed: BTreeSet::new(),
            modes: BTreeMap::new(),
            symlinks: BTreeMap::new(),
        }
    }

    /// The pack root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Replaces the whole manifest text.
    #[must_use]
    pub fn manifest(mut self, text: impl Into<String>) -> Self {
        self.manifest = text.into();
        self.raw_manifest = None;
        self
    }

    /// Replaces one substring in the manifest, failing loudly at build time when absent.
    #[must_use]
    pub fn replace(mut self, from: &str, to: &str) -> Self {
        assert!(
            self.manifest.contains(from),
            "fixture replacement `{from}` is not in the manifest"
        );
        self.manifest = self.manifest.replace(from, to);
        self
    }

    /// Appends a manifest fragment (a new section, usually).
    #[must_use]
    pub fn append(mut self, fragment: &str) -> Self {
        self.manifest.push_str(fragment);
        self
    }

    /// Writes raw manifest bytes, bypassing TOML construction (encoding/size fixtures).
    #[must_use]
    pub fn raw_manifest(mut self, bytes: Vec<u8>) -> Self {
        self.raw_manifest = Some(bytes);
        self
    }

    /// Adds or replaces a pack-relative file.
    #[must_use]
    pub fn file(mut self, path: &str, contents: impl Into<Vec<u8>>) -> Self {
        self.removed.remove(path);
        self.files.insert(path.to_owned(), contents.into());
        self
    }

    /// Removes a default file from the tree.
    #[must_use]
    pub fn remove_file(mut self, path: &str) -> Self {
        self.files.remove(path);
        self.removed.insert(path.to_owned());
        self
    }

    /// Applies a Unix mode to a pack-relative file at build time.
    #[must_use]
    pub fn mode(mut self, path: &str, mode: u32) -> Self {
        self.modes.insert(path.to_owned(), mode);
        self
    }

    /// Adds a symlink at a pack-relative path.
    #[must_use]
    pub fn symlink(mut self, path: &str, target: impl Into<PathBuf>) -> Self {
        self.symlinks.insert(path.to_owned(), target.into());
        self
    }

    /// Writes the tree and returns its root.
    ///
    /// # Panics
    ///
    /// Panics on filesystem failure: this is test support, not library code.
    pub fn build(&self) -> &Path {
        std::fs::create_dir_all(&self.root).expect("fixture root");
        for (relative, contents) in &self.files {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("fixture parent");
            }
            std::fs::write(&path, contents).expect("fixture file");
        }
        let manifest_path = self.root.join("pack.toml");
        match &self.raw_manifest {
            Some(bytes) => std::fs::write(&manifest_path, bytes).expect("raw manifest"),
            None => std::fs::write(&manifest_path, self.manifest.as_bytes()).expect("manifest"),
        }
        for (relative, mode) in &self.modes {
            set_mode(&self.root.join(relative), *mode);
        }
        for (relative, target) in &self.symlinks {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("symlink parent");
            }
            make_symlink(target, &path);
        }
        &self.root
    }
}

/// Writes a `sha256sum`-compatible checksum manifest for the current tree.
///
/// `mutate` runs after the checksum lines are computed, so a test can corrupt one file
/// after its correct digest was recorded.
pub fn write_checksums(root: &Path, skip: &[&str], mutate: impl FnOnce()) -> PathBuf {
    let snapshot = tree::snapshot(root).expect("snapshot for checksums");
    let mut lines = String::new();
    for (relative, entry) in snapshot.files() {
        if relative == "checksums.sha256" || skip.contains(&relative) {
            continue;
        }
        if entry.kind != EntryKind::File {
            continue;
        }
        let bytes = std::fs::read(root.join(relative)).expect("checksum read");
        let digest = sha256(&bytes);
        lines.push_str(&format!("{digest}  {relative}\n"));
    }
    mutate();
    let path = root.join("checksums.sha256");
    std::fs::write(&path, lines).expect("checksums file");
    path
}

/// The canonical SHA-256 hex digest used by the checksum fixtures.
#[must_use]
pub fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("fixture mode");
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

#[cfg(unix)]
fn make_symlink(target: &Path, path: &Path) {
    std::os::unix::fs::symlink(target, path).expect("fixture symlink");
}

#[cfg(not(unix))]
fn make_symlink(_target: &Path, _path: &Path) {
    panic!("symlink fixtures require a Unix host");
}
