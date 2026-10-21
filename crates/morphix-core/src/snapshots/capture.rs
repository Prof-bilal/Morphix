//! Pre-state capture for file-backed resources (P4-07/P4-08).
//!
//! **Ownership.** Turns a [`CaptureSpec`] — concrete host paths plus key identities —
//! into a sealed [`SnapshotManifest`] with every blob in the [`ObjectStore`]. This is
//! the only producer of snapshot content; readers trust only sealed, verified
//! manifests.
//!
//! **Scope.** Capture handles exactly the file-backed resources: whole config files,
//! per-key values, asset files and directories, and the wallpaper descriptor read
//! from appletsrc. Live desktop state (panels, widgets, shortcuts, effects) needs
//! the Phase 6 adapter reads and is refused at plan time until then (ADR-024); it is
//! not representable here on purpose.
//!
//! **Whole-file rule (TRANSACTIONS.md §6.2).** Every config file named by a
//! [`CaptureEntry::ConfigKey`] and the appletsrc named by
//! [`CaptureEntry::Wallpaper`] are captured whole in addition to the key or
//! descriptor, deduplicated: capture appends the file entry when the spec does not
//! already contain one. Whole files make manual recovery possible; per-key values
//! make conditional rollback precise.
//!
//! [`ObjectStore`]: super::store::ObjectStore

use std::collections::BTreeSet;

use crate::desktop::kde::{config, wallpaper};
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::fs::sandbox::SafePath;
use crate::fs::tree::{self, EntryKind};
use crate::fs::{FileSystem, MAX_READ_BYTES};

use super::manifest::{PreState, ResourceRef, SnapshotEntry, SnapshotManifest, SystemInfo};
use super::store::ObjectStore;

/// What to capture. Paths are concrete host paths: resolving plan targets through
/// the fixed target map is the caller's job (Phase 5), so this layer never invents
/// a KDE path and never depends on planning types.
#[derive(Debug, Clone)]
pub enum CaptureEntry {
    /// A whole config file.
    ConfigFile {
        /// Absolute path of the file.
        path: SafePath,
    },
    /// One key inside a config file. The whole file is captured too (see above).
    ConfigKey {
        /// Config file identity (`kdeglobals`, `kwinrc`, …).
        file: String,
        /// Absolute path of the file.
        path: SafePath,
        /// Group within the file.
        group: String,
        /// Key within the group.
        key: String,
    },
    /// A single asset file.
    AssetFile {
        /// Absolute installed path.
        path: SafePath,
    },
    /// A whole asset directory tree, captured per file plus a tree record.
    AssetDir {
        /// Absolute installed path.
        path: SafePath,
    },
    /// The wallpaper descriptor for one desktop, read from appletsrc.
    Wallpaper {
        /// Desktop identity, or `all`.
        desktop: String,
        /// Absolute path of the appletsrc file.
        appletsrc: SafePath,
    },
}

/// Everything capture needs: identities, the output directory, and the entries.
#[derive(Debug, Clone)]
pub struct CaptureSpec {
    /// Owning snapshot id (UUIDv7 string).
    pub snapshot_id: String,
    /// Owning transaction id (UUIDv7 string).
    pub transaction_id: String,
    /// Creation time, RFC 3339 UTC.
    pub created_at: String,
    /// Writing build version.
    pub app_version: String,
    /// Detected Plasma version, when known.
    pub plasma_version: Option<String>,
    /// Session type (`wayland`, `x11`, …).
    pub session: String,
    /// Snapshot directory; created `0o700`, holds `objects/` and `snapshot.json`.
    pub snapshot_dir: SafePath,
    /// Resources to capture, in order.
    pub entries: Vec<CaptureEntry>,
}

/// Captures every requested pre-state, seals the manifest, and writes it.
///
/// Missing file-backed targets are recorded as [`PreState::Absent`], never as
/// errors — an absent target is exactly what a create operation needs to restore.
/// Symlinked targets are refused with `E_SYMLINK_TARGET` before any read.
///
/// # Errors
///
/// Returns a typed I/O or snapshot error when a present target cannot be captured.
pub fn capture_snapshot<F: FileSystem>(fs: &F, spec: &CaptureSpec) -> Result<SnapshotManifest> {
    fs.create_dir_all(&spec.snapshot_dir, 0o700)?;
    let store = ObjectStore::open(fs, &spec.snapshot_dir)?;

    let mut entries = Vec::with_capacity(spec.entries.len());
    let mut filed = BTreeSet::new();
    for request in &spec.entries {
        match request {
            CaptureEntry::ConfigFile { path } => {
                filed.insert(path.as_path().display().to_string());
                entries.push(capture_config_file(fs, &store, path)?);
            }
            CaptureEntry::ConfigKey {
                file,
                path,
                group,
                key,
            } => {
                entries.push(capture_config_key(fs, path, file, group, key)?);
            }
            CaptureEntry::AssetFile { path } => {
                entries.push(capture_asset_file(fs, &store, path)?);
            }
            CaptureEntry::AssetDir { path } => {
                entries.extend(capture_asset_dir(fs, &store, path)?);
            }
            CaptureEntry::Wallpaper { desktop, appletsrc } => {
                entries.push(capture_wallpaper(fs, appletsrc, desktop)?);
            }
        }
    }

    // Whole-file rule: every referenced config file and the appletsrc travel with
    // the snapshot even when only a key or descriptor was requested.
    let mut whole_files: Vec<SafePath> = Vec::new();
    for request in &spec.entries {
        let path = match request {
            CaptureEntry::ConfigKey { path, .. }
            | CaptureEntry::Wallpaper {
                appletsrc: path, ..
            } => path,
            CaptureEntry::ConfigFile { .. }
            | CaptureEntry::AssetFile { .. }
            | CaptureEntry::AssetDir { .. } => continue,
        };
        if filed.insert(path.as_path().display().to_string()) {
            whole_files.push(path.clone());
        }
    }
    for path in &whole_files {
        entries.push(capture_config_file(fs, &store, path)?);
    }

    let manifest = SnapshotManifest::new(
        &spec.snapshot_id,
        &spec.transaction_id,
        &spec.created_at,
        &spec.app_version,
        SystemInfo {
            plasma_version: spec.plasma_version.clone(),
            session: spec.session.clone(),
        },
        entries,
    )?;
    let sealed = manifest.seal()?;
    sealed.write_to(fs, &spec.snapshot_dir)?;
    Ok(sealed)
}

/// Refuses symlinked ancestors before any read (SECURITY.md §8, T-18).
fn check_ancestors(path: &SafePath) -> Result<()> {
    path.require_no_symlink_ancestors()
}

fn capture_config_file<F: FileSystem>(
    fs: &F,
    store: &ObjectStore<'_, F>,
    path: &SafePath,
) -> Result<SnapshotEntry> {
    let resource = ResourceRef::ConfigFile {
        path: path.as_path().display().to_string(),
    };
    match present_file(path, fs)? {
        None => Ok(SnapshotEntry {
            resource,
            pre_state: PreState::Absent,
            object: None,
        }),
        Some((mode, size)) => {
            let hash = store.put_file(path)?;
            Ok(SnapshotEntry {
                resource,
                pre_state: PreState::File {
                    sha256: hash.as_str().to_owned(),
                    mode,
                    size,
                },
                object: Some(hash.as_str().to_owned()),
            })
        }
    }
}

fn capture_config_key<F: FileSystem>(
    fs: &F,
    path: &SafePath,
    file: &str,
    group: &str,
    key: &str,
) -> Result<SnapshotEntry> {
    check_ancestors(path)?;
    let resource = ResourceRef::ConfigKey {
        file: file.to_owned(),
        group: group.to_owned(),
        key: key.to_owned(),
    };
    if fs.metadata(path).is_err() {
        return Ok(SnapshotEntry {
            resource,
            pre_state: PreState::Absent,
            object: None,
        });
    }
    let bytes = fs.read_file(path, MAX_READ_BYTES)?;
    match config::read_value(&bytes, group, key)? {
        None => Ok(SnapshotEntry {
            resource,
            pre_state: PreState::Absent,
            object: None,
        }),
        Some(value) => Ok(SnapshotEntry {
            resource,
            pre_state: PreState::Value { value },
            object: None,
        }),
    }
}

fn capture_asset_file<F: FileSystem>(
    fs: &F,
    store: &ObjectStore<'_, F>,
    path: &SafePath,
) -> Result<SnapshotEntry> {
    let resource = ResourceRef::AssetFile {
        path: path.as_path().display().to_string(),
    };
    match present_file(path, fs)? {
        None => Ok(SnapshotEntry {
            resource,
            pre_state: PreState::Absent,
            object: None,
        }),
        Some((mode, size)) => {
            let hash = store.put_file(path)?;
            Ok(SnapshotEntry {
                resource,
                pre_state: PreState::File {
                    sha256: hash.as_str().to_owned(),
                    mode,
                    size,
                },
                object: Some(hash.as_str().to_owned()),
            })
        }
    }
}

fn capture_asset_dir<F: FileSystem>(
    fs: &F,
    store: &ObjectStore<'_, F>,
    path: &SafePath,
) -> Result<Vec<SnapshotEntry>> {
    let resource = ResourceRef::AssetDir {
        path: path.as_path().display().to_string(),
    };
    check_ancestors(path)?;
    match fs.metadata(path) {
        Err(error) if error.code == ErrorCode::Io => {
            return Ok(vec![SnapshotEntry {
                resource,
                pre_state: PreState::Absent,
                object: None,
            }]);
        }
        Err(error) => return Err(error),
        Ok(metadata) if metadata.kind == EntryKind::Symlink => {
            return Err(symlink_target(path));
        }
        Ok(metadata) if metadata.kind != EntryKind::Directory => {
            return Err(
                MorphixError::new(ErrorCode::Io, "a filesystem operation failed")
                    .with_context("path", path.as_path().display().to_string())
                    .with_context("io_kind", "NotADirectory"),
            );
        }
        Ok(_) => {}
    }
    let tree = tree::snapshot(path.as_path())?;
    let mut entries = Vec::with_capacity(tree.file_count() + 1);
    for tree_entry in &tree.entries {
        match tree_entry.kind {
            EntryKind::File => {
                let file_path = path.join_relative(&tree_entry.relative).map_err(|error| {
                    MorphixError::new(ErrorCode::Internal, "a tree entry escaped its root")
                        .with_context("reason", error.to_string())
                })?;
                // Ancestors inside the tree were just snapshotted without following
                // links, but each file still passes the ancestor check on its own:
                // a link swapped in between the snapshot and this read is refused.
                check_ancestors(&file_path)?;
                let bytes = fs.read_file(&file_path, MAX_READ_BYTES)?;
                let hash = store.put(&bytes)?;
                entries.push(SnapshotEntry {
                    resource: ResourceRef::AssetFile {
                        path: file_path.as_path().display().to_string(),
                    },
                    pre_state: PreState::File {
                        sha256: hash.as_str().to_owned(),
                        mode: tree_entry.mode,
                        size: tree_entry.size,
                    },
                    object: Some(hash.as_str().to_owned()),
                });
            }
            EntryKind::Directory => {}
            EntryKind::Symlink | EntryKind::Other => {
                return Err(symlink_target(path));
            }
        }
    }
    entries.insert(
        0,
        SnapshotEntry {
            resource,
            pre_state: PreState::Dir {
                tree_hash: tree.content_hash(),
                files: tree.file_count() as u64,
                bytes: tree.total_bytes,
            },
            object: None,
        },
    );
    Ok(entries)
}

fn capture_wallpaper<F: FileSystem>(
    fs: &F,
    appletsrc: &SafePath,
    desktop: &str,
) -> Result<SnapshotEntry> {
    let resource = ResourceRef::Wallpaper {
        desktop: desktop.to_owned(),
    };
    check_ancestors(appletsrc)?;
    if fs.metadata(appletsrc).is_err() {
        return Ok(SnapshotEntry {
            resource,
            pre_state: PreState::Absent,
            object: None,
        });
    }
    let bytes = fs.read_file(appletsrc, MAX_READ_BYTES)?;
    match wallpaper::read_image_wallpaper(&bytes)? {
        None => Ok(SnapshotEntry {
            resource,
            pre_state: PreState::Absent,
            object: None,
        }),
        Some(descriptor) => Ok(SnapshotEntry {
            resource,
            pre_state: PreState::Wallpaper {
                plugin: descriptor.plugin,
                image: descriptor.image,
            },
            object: None,
        }),
    }
}

/// A present regular file, as `(mode, size)`; missing files are `None`. Symlinks
/// and directories are refused: capture never reads through a link and never
/// treats a directory as a file.
fn present_file<F: FileSystem>(path: &SafePath, fs: &F) -> Result<Option<(u32, u64)>> {
    check_ancestors(path)?;
    match fs.metadata(path) {
        Err(error) if error.code == ErrorCode::Io => Ok(None),
        Err(error) => Err(error),
        Ok(metadata) if metadata.kind == EntryKind::Symlink => Err(symlink_target(path)),
        Ok(metadata) if metadata.kind != EntryKind::File => Err(MorphixError::new(
            ErrorCode::Io,
            "a filesystem operation failed",
        )
        .with_context("path", path.as_path().display().to_string())
        .with_context("io_kind", "IsADirectory")),
        Ok(metadata) => Ok(Some((metadata.mode & 0o7777, metadata.len))),
    }
}

fn symlink_target(path: &SafePath) -> MorphixError {
    MorphixError::new(
        ErrorCode::SymlinkTarget,
        "Morphix refuses to capture through a symbolic link",
    )
    .with_context("path", path.as_path().display().to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{CaptureEntry, CaptureSpec, capture_snapshot};
    use crate::fs::sandbox::SafePath;
    use crate::fs::{FileSystem, RealFileSystem};
    use crate::snapshots::manifest::{PreState, ResourceRef};

    fn spec(snapshot_dir: SafePath, entries: Vec<CaptureEntry>) -> CaptureSpec {
        CaptureSpec {
            snapshot_id: "01900000-0000-7000-8000-000000000001".to_owned(),
            transaction_id: "01900000-0000-7000-8000-000000000002".to_owned(),
            created_at: "2026-10-21T00:00:00Z".to_owned(),
            app_version: "0.1.0".to_owned(),
            plasma_version: Some("6.3.5".to_owned()),
            session: "wayland".to_owned(),
            snapshot_dir,
            entries,
        }
    }

    fn safe(root: &std::path::Path, name: &str) -> SafePath {
        SafePath::new(root.join(name)).expect("safe path")
    }

    #[test]
    fn captures_keys_files_and_appends_whole_files_once() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let kdeglobals = safe(directory.path(), "kdeglobals");
        fs.write_file_atomic(&kdeglobals, b"[General]\nColorScheme=BreezeDark\n", 0o644)
            .expect("write");

        let manifest = capture_snapshot(
            &fs,
            &spec(
                safe(directory.path(), "snap"),
                vec![
                    CaptureEntry::ConfigKey {
                        file: "kdeglobals".to_owned(),
                        path: kdeglobals.clone(),
                        group: "General".to_owned(),
                        key: "ColorScheme".to_owned(),
                    },
                    CaptureEntry::ConfigKey {
                        file: "kdeglobals".to_owned(),
                        path: kdeglobals.clone(),
                        group: "General".to_owned(),
                        key: "Missing".to_owned(),
                    },
                ],
            ),
        )
        .expect("capture");

        manifest.verify().expect("sealed manifest verifies");
        assert_eq!(manifest.entries.len(), 3, "two keys plus one whole file");
        assert!(matches!(
            &manifest.entries[0].pre_state,
            PreState::Value { value } if value == "BreezeDark"
        ));
        assert!(matches!(&manifest.entries[1].pre_state, PreState::Absent));
        assert!(matches!(
            &manifest.entries[2].resource,
            ResourceRef::ConfigFile { .. }
        ));
        assert!(matches!(
            &manifest.entries[2].pre_state,
            PreState::File { .. }
        ));
    }

    #[test]
    fn missing_targets_are_absent_not_errors() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let manifest = capture_snapshot(
            &fs,
            &spec(
                safe(directory.path(), "snap"),
                vec![
                    CaptureEntry::ConfigFile {
                        path: safe(directory.path(), "nope"),
                    },
                    CaptureEntry::AssetFile {
                        path: safe(directory.path(), "nope.png"),
                    },
                    CaptureEntry::AssetDir {
                        path: safe(directory.path(), "nope.d"),
                    },
                    CaptureEntry::Wallpaper {
                        desktop: "all".to_owned(),
                        appletsrc: safe(directory.path(), "appletsrc"),
                    },
                ],
            ),
        )
        .expect("capture");
        assert!(
            manifest
                .entries
                .iter()
                .all(|entry| matches!(entry.pre_state, PreState::Absent)),
            "every missing target records Absent"
        );
    }

    #[test]
    fn captures_asset_trees_per_file_with_a_tree_record() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let root = safe(directory.path(), "icons");
        let sub = root.join_relative("scalable").expect("sub");
        fs.create_dir_all(&sub, 0o755).expect("mkdir");
        let one = root.join_relative("index.theme").expect("one");
        let two = sub.join_relative("icon.svg").expect("two");
        fs.write_file_atomic(&one, b"[Icon Theme]\n", 0o644)
            .expect("write one");
        fs.write_file_atomic(&two, b"<svg/>\n", 0o644)
            .expect("write two");

        let manifest = capture_snapshot(
            &fs,
            &spec(
                safe(directory.path(), "snap"),
                vec![CaptureEntry::AssetDir { path: root.clone() }],
            ),
        )
        .expect("capture");

        assert_eq!(
            manifest.entries.len(),
            3,
            "one Dir record plus two file entries"
        );
        let (dir, files) = manifest.entries.split_first().expect("entries");
        assert!(matches!(dir.resource, ResourceRef::AssetDir { .. }));
        let PreState::Dir { files: count, .. } = &dir.pre_state else {
            panic!("first entry must be the Dir record");
        };
        assert_eq!(*count, 2);
        assert!(dir.object.is_none());
        for entry in files {
            assert!(matches!(entry.resource, ResourceRef::AssetFile { .. }));
            assert!(matches!(entry.pre_state, PreState::File { .. }));
            assert!(entry.object.is_some());
        }
    }

    #[test]
    fn symlinked_targets_are_refused_before_any_read() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let real = safe(directory.path(), "real");
        fs.write_file_atomic(&real, b"real", 0o644).expect("write");
        let link = safe(directory.path(), "link");
        std::os::unix::fs::symlink(real.as_path(), link.as_path()).expect("symlink");

        let error = capture_snapshot(
            &fs,
            &spec(
                safe(directory.path(), "snap"),
                vec![CaptureEntry::AssetFile { path: link }],
            ),
        )
        .expect_err("symlink refused");
        assert_eq!(error.code, crate::errors::ErrorCode::SymlinkTarget);
    }

    #[test]
    fn captures_wallpaper_descriptors_with_their_file() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let appletsrc = safe(directory.path(), "appletsrc");
        fs.write_file_atomic(
            &appletsrc,
            b"[Containments][2][Wallpaper][org.kde.image][General]\nImage=file:///pic.jpg\n",
            0o644,
        )
        .expect("write");

        let manifest = capture_snapshot(
            &fs,
            &spec(
                safe(directory.path(), "snap"),
                vec![CaptureEntry::Wallpaper {
                    desktop: "all".to_owned(),
                    appletsrc: appletsrc.clone(),
                }],
            ),
        )
        .expect("capture");

        assert_eq!(
            manifest.entries.len(),
            2,
            "descriptor plus the appletsrc file"
        );
        assert!(matches!(
            &manifest.entries[0].pre_state,
            PreState::Wallpaper { image, .. } if image == "file:///pic.jpg"
        ));
        assert!(matches!(
            &manifest.entries[1].resource,
            ResourceRef::ConfigFile { .. }
        ));
    }
}
