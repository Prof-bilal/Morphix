//! Snapshot-restore plan generation (P4-11, MVP AC-16).
//!
//! **Ownership.** Turns a verified snapshot into an exact, ordered restore plan with
//! drift warnings. This is planning, not applying: nothing is read except the
//! snapshot and the live pre-state, and nothing is written.
//!
//! **Rules.**
//!
//! - The snapshot is verified first via [`verify_snapshot`]. A damaged snapshot is
//!   refused with `E_SNAPSHOT_UNREADABLE`/`E_SNAPSHOT_CORRUPT`; there is no plan
//!   from a snapshot that does not verify (TESTING.md §4.3).
//! - Whole-file entries drive file restores; per-key entries drive drift warnings
//!   only. Both exist because rollback needs conditional per-key precision while
//!   restore needs exact files (TRANSACTIONS.md §6.2).
//! - Live state that differs from the recorded pre-state produces a [`DriftWarning`],
//!   never a refusal: restore is an explicit return to recorded state, and the
//!   warnings are what make that choice informed. The executor (Phase 5) decides
//!   how warnings gate application.
//! - Operation order is deterministic: manifest order, with a directory's per-file
//!   restores grouped under its tree restore.

use crate::desktop::kde::config;
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::fs::sandbox::SafePath;
use crate::fs::tree::{self, EntryKind};
use crate::fs::{FileSystem, MAX_READ_BYTES};

use super::manifest::{PreState, ResourceRef, SnapshotManifest};
use super::store::ObjectHash;
use super::verify::verify_snapshot;

/// One exact restore step for the Phase 5 executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOp {
    /// Write object bytes to `path` with `mode`.
    RestoreFile {
        /// Absolute target path.
        path: String,
        /// Object hash in the snapshot store.
        object: String,
        /// Permission bits to apply.
        mode: u32,
    },
    /// Replace the tree at `path` with the recorded files (verified by `tree_hash`
    /// after the executor rebuilds it).
    RestoreDir {
        /// Absolute target path.
        path: String,
        /// Recorded deterministic tree hash.
        tree_hash: String,
        /// Per-file restores, in manifest order.
        files: Vec<DirFile>,
    },
    /// Remove `path`: it did not exist at capture. `live_existed` records whether
    /// anything was actually there to remove when the plan was made.
    RemovePath {
        /// Absolute target path.
        path: String,
        /// Whether the path existed when the plan was made.
        live_existed: bool,
    },
    /// Leave `path` alone: the recorded and live states already agree.
    Noop {
        /// Absolute target path.
        path: String,
    },
    /// Set (`Some`) or delete (`None`) a config key in `path`.
    RestoreKey {
        /// Config file identity.
        file: String,
        /// Absolute path of the file.
        path: String,
        /// Group within the file.
        group: String,
        /// Key within the group.
        key: String,
        /// Recorded value; `None` deletes a key that was absent at capture.
        value: Option<String>,
    },
    /// Restore a wallpaper descriptor.
    RestoreWallpaper {
        /// Desktop identity, or `all`.
        desktop: String,
        /// Recorded plugin id.
        plugin: String,
        /// Recorded image URI.
        image: String,
    },
}

/// One file inside a [`RestoreOp::RestoreDir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirFile {
    /// Path relative to the tree root, `/`-separated.
    pub relative: String,
    /// Object hash in the snapshot store.
    pub object: String,
    /// Permission bits to apply.
    pub mode: u32,
}

/// How the live state differs from the recorded pre-state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftKind {
    /// A recorded value no longer matches the live value.
    ValueChanged,
    /// A recorded file's content no longer matches.
    FileChanged,
    /// A recorded tree no longer matches.
    TreeChanged,
    /// The path did not exist at capture but exists now.
    CreatedSince,
    /// The live target could not be read (for example a symlink).
    LiveUnreadable,
}

/// A non-blocking drift note: what changed since capture, in user terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftWarning {
    /// What drifted: config identity, path, or `wallpaper:<desktop>`.
    pub resource: String,
    /// How it drifted.
    pub kind: DriftKind,
    /// One-sentence detail, without file contents.
    pub detail: String,
}

/// An exact restore plan with its drift warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlan {
    /// Owning snapshot id.
    pub snapshot_id: String,
    /// Owning transaction id.
    pub transaction_id: String,
    /// Ordered restore steps.
    pub ops: Vec<RestoreOp>,
    /// Non-blocking drift notes.
    pub warnings: Vec<DriftWarning>,
}

/// Builds the restore plan for a snapshot directory.
///
/// Verifies first, then compares every entry against the live state. Damaged
/// snapshots are refused; drifted live state is warned about and still planned.
///
/// `resolve_config` maps a config file identity (`kdeglobals`, …) to its live
/// host path for drift comparison. It is a callback rather than a constructed
/// path because only the fixed target map (Phase 5) knows KDE locations; when it
/// returns `None`, the key op is still exact and the warning says the live value
/// is unknown.
///
/// # Errors
///
/// Returns `E_SNAPSHOT_UNREADABLE`/`E_SNAPSHOT_CORRUPT` for a snapshot that does
/// not verify, and a typed I/O error when the live state cannot be inspected.
pub fn plan_restore<F: FileSystem>(
    fs: &F,
    snapshot_dir: &SafePath,
    resolve_config: &dyn Fn(&str) -> Option<SafePath>,
) -> Result<RestorePlan> {
    let report = verify_snapshot(fs, snapshot_dir)?;
    let manifest = report.manifest;
    let planner = RestorePlanner { fs, resolve_config };
    let mut ops = Vec::new();
    let mut warnings = Vec::new();
    let mut index = 0;
    while index < manifest.entries.len() {
        let entry = &manifest.entries[index];
        if let ResourceRef::AssetDir { path } = &entry.resource
            && let PreState::Dir { tree_hash, .. } = &entry.pre_state
        {
            let (op, files_used) =
                planner.plan_dir(&manifest, index, path, tree_hash, &mut warnings)?;
            ops.push(op);
            index += files_used;
            continue;
        }
        ops.push(planner.plan_entry(entry, &mut warnings)?);
        index += 1;
    }
    Ok(RestorePlan {
        snapshot_id: manifest.snapshot_id.clone(),
        transaction_id: manifest.transaction_id.clone(),
        ops,
        warnings,
    })
}

struct RestorePlanner<'a, F: FileSystem> {
    fs: &'a F,
    resolve_config: &'a dyn Fn(&str) -> Option<SafePath>,
}

impl<F: FileSystem> RestorePlanner<'_, F> {
    /// Plans a directory restore, consuming the Dir entry plus the consecutive
    /// per-file AssetFile entries capture emitted for it.
    fn plan_dir(
        &self,
        manifest: &SnapshotManifest,
        at: usize,
        path: &str,
        tree_hash: &str,
        warnings: &mut Vec<DriftWarning>,
    ) -> Result<(RestoreOp, usize)> {
        let mut files = Vec::new();
        let mut consumed = 1;
        for entry in manifest.entries.iter().skip(at + 1) {
            let (file_path, object, mode) = match (&entry.resource, &entry.pre_state) {
                (ResourceRef::AssetFile { path: file_path }, PreState::File { mode, .. })
                    if file_path.starts_with(path) && file_path[path.len()..].starts_with('/') =>
                {
                    (file_path, entry.object.as_ref(), *mode)
                }
                _ => break,
            };
            let Some(object) = object else {
                return Err(MorphixError::new(
                    ErrorCode::SnapshotCorrupt,
                    "a captured tree file has no object",
                )
                .with_context("snapshot_id", &manifest.snapshot_id));
            };
            ObjectHash::parse(object)?;
            let relative = file_path[path.len() + 1..].to_owned();
            files.push(DirFile {
                relative,
                object: object.clone(),
                mode,
            });
            consumed += 1;
        }
        let live = self.live_tree(path);
        match live {
            LiveTree::Missing => {}
            LiveTree::Present {
                tree_hash: live_hash,
            } if live_hash == tree_hash => {}
            LiveTree::Present { .. } => warnings.push(DriftWarning {
                resource: path.to_owned(),
                kind: DriftKind::TreeChanged,
                detail: "the directory changed since the snapshot; restore replaces it".to_owned(),
            }),
            LiveTree::Unreadable => warnings.push(DriftWarning {
                resource: path.to_owned(),
                kind: DriftKind::LiveUnreadable,
                detail: "the live directory cannot be inspected; restore replaces it".to_owned(),
            }),
        }
        Ok((
            RestoreOp::RestoreDir {
                path: path.to_owned(),
                tree_hash: tree_hash.to_owned(),
                files,
            },
            consumed,
        ))
    }

    fn plan_entry(
        &self,
        entry: &super::manifest::SnapshotEntry,
        warnings: &mut Vec<DriftWarning>,
    ) -> Result<RestoreOp> {
        match (&entry.resource, &entry.pre_state) {
            (ResourceRef::ConfigFile { path }, PreState::File { mode, .. }) => {
                let object = required_object(entry)?;
                self.plan_file(path, object, *mode, warnings)
            }
            (ResourceRef::ConfigFile { path }, PreState::Absent) => {
                Ok(self.plan_absent(path, "the file", warnings))
            }
            (ResourceRef::ConfigKey { file, group, key }, pre_state) => {
                Ok(self.plan_key(file, group, key, pre_state, warnings))
            }
            (ResourceRef::AssetFile { path }, PreState::File { mode, .. }) => {
                let object = required_object(entry)?;
                self.plan_file(path, object, *mode, warnings)
            }
            (ResourceRef::AssetFile { path }, PreState::Absent) => {
                Ok(self.plan_absent(path, "the file", warnings))
            }
            (ResourceRef::AssetDir { .. }, _) => Err(MorphixError::new(
                ErrorCode::Internal,
                "a directory entry escaped its tree group",
            )),
            (ResourceRef::Wallpaper { desktop }, PreState::Wallpaper { plugin, image }) => {
                Ok(plan_wallpaper(desktop, plugin, image))
            }
            (ResourceRef::Wallpaper { desktop }, PreState::Absent) => Ok(RestoreOp::Noop {
                path: format!("wallpaper:{desktop}"),
            }),
            _ => Err(MorphixError::new(
                ErrorCode::SnapshotCorrupt,
                "a manifest entry pairs a resource with an impossible pre-state",
            )),
        }
    }

    fn plan_file(
        &self,
        path: &str,
        object: &str,
        mode: u32,
        warnings: &mut Vec<DriftWarning>,
    ) -> Result<RestoreOp> {
        let safe = SafePath::new(path.to_owned()).map_err(|_| {
            MorphixError::new(ErrorCode::SnapshotCorrupt, "a manifest path is not safe")
                .with_context("path", path)
        })?;
        match self.fs.metadata(&safe) {
            Err(error) if error.code == ErrorCode::Io => Ok(RestoreOp::RestoreFile {
                path: path.to_owned(),
                object: object.to_owned(),
                mode,
            }),
            Err(error) => Err(error),
            Ok(metadata) if metadata.kind == EntryKind::Symlink => {
                warnings.push(DriftWarning {
                    resource: path.to_owned(),
                    kind: DriftKind::LiveUnreadable,
                    detail: "the live path is a symlink; restore refuses links at apply time"
                        .to_owned(),
                });
                Ok(RestoreOp::RestoreFile {
                    path: path.to_owned(),
                    object: object.to_owned(),
                    mode,
                })
            }
            Ok(metadata) if metadata.kind != EntryKind::File => {
                warnings.push(DriftWarning {
                    resource: path.to_owned(),
                    kind: DriftKind::FileChanged,
                    detail: "the live path is not a regular file; restore replaces it".to_owned(),
                });
                Ok(RestoreOp::RestoreFile {
                    path: path.to_owned(),
                    object: object.to_owned(),
                    mode,
                })
            }
            Ok(_) => {
                // The object hash was validated when the snapshot verified, so a
                // direct comparison is sound here.
                let live_hash = tree::hash_file(safe.as_path())?;
                if live_hash == *object {
                    Ok(RestoreOp::Noop {
                        path: path.to_owned(),
                    })
                } else {
                    warnings.push(DriftWarning {
                        resource: path.to_owned(),
                        kind: DriftKind::FileChanged,
                        detail: "the file changed since the snapshot; restore overwrites it"
                            .to_owned(),
                    });
                    Ok(RestoreOp::RestoreFile {
                        path: path.to_owned(),
                        object: object.to_owned(),
                        mode,
                    })
                }
            }
        }
    }

    fn plan_absent(&self, path: &str, what: &str, warnings: &mut Vec<DriftWarning>) -> RestoreOp {
        let live_existed = SafePath::new(path.to_owned())
            .ok()
            .is_some_and(|safe| self.fs.metadata(&safe).is_ok());
        if live_existed {
            warnings.push(DriftWarning {
                resource: path.to_owned(),
                kind: DriftKind::CreatedSince,
                detail: format!("{what} did not exist at capture; restore removes it"),
            });
        }
        if live_existed {
            RestoreOp::RemovePath {
                path: path.to_owned(),
                live_existed: true,
            }
        } else {
            RestoreOp::Noop {
                path: path.to_owned(),
            }
        }
    }

    fn plan_key(
        &self,
        file: &str,
        group: &str,
        key: &str,
        pre_state: &PreState,
        warnings: &mut Vec<DriftWarning>,
    ) -> RestoreOp {
        let expected = match pre_state {
            PreState::Value { value } => Some(value.clone()),
            PreState::Absent => None,
            _ => {
                warnings.push(DriftWarning {
                    resource: format!("{file}[{group}]/{key}"),
                    kind: DriftKind::LiveUnreadable,
                    detail: "the key entry is malformed; the file restore covers it".to_owned(),
                });
                return RestoreOp::Noop {
                    path: format!("{file}[{group}]/{key}"),
                };
            }
        };
        // The manifest records the file identity, not the host path: resolving the
        // identity to a path is the caller's job (fixed target map, Phase 5). When
        // the identity does not resolve here, the op still records the exact
        // restore and the drift warning says the live value is unknown.
        let live = (self.resolve_config)(file).and_then(|safe| {
            self.fs
                .read_file(&safe, MAX_READ_BYTES)
                .ok()
                .and_then(|bytes| config::read_value(&bytes, group, key).ok().flatten())
        });
        if (self.resolve_config)(file).is_none() {
            warnings.push(DriftWarning {
                resource: format!("{file}[{group}]/{key}"),
                kind: DriftKind::LiveUnreadable,
                detail: "the config file location is unknown; the file restore covers it"
                    .to_owned(),
            });
        } else if live != expected {
            warnings.push(DriftWarning {
                resource: format!("{file}[{group}]/{key}"),
                kind: DriftKind::ValueChanged,
                detail: "the key changed since the snapshot; the file restore covers it".to_owned(),
            });
        }
        RestoreOp::RestoreKey {
            file: file.to_owned(),
            path: (self.resolve_config)(file)
                .map(|safe| safe.as_path().display().to_string())
                .unwrap_or_default(),
            group: group.to_owned(),
            key: key.to_owned(),
            value: expected,
        }
    }

    fn live_tree(&self, path: &str) -> LiveTree {
        let Ok(safe) = SafePath::new(path.to_owned()) else {
            return LiveTree::Unreadable;
        };
        match self.fs.metadata(&safe) {
            Err(error) if error.code == ErrorCode::Io => LiveTree::Missing,
            Err(_) => LiveTree::Unreadable,
            Ok(metadata) if metadata.kind != EntryKind::Directory => LiveTree::Unreadable,
            Ok(_) => match tree::snapshot(safe.as_path()) {
                Ok(tree) => LiveTree::Present {
                    tree_hash: tree.content_hash(),
                },
                Err(_) => LiveTree::Unreadable,
            },
        }
    }
}

enum LiveTree {
    Missing,
    Present { tree_hash: String },
    Unreadable,
}

fn required_object(entry: &super::manifest::SnapshotEntry) -> Result<&str> {
    entry.object.as_deref().ok_or_else(|| {
        MorphixError::new(ErrorCode::SnapshotCorrupt, "a captured file has no object")
    })
}

/// Builds the descriptor op. Wallpaper drift is covered by the appletsrc file entry
/// capture always appends: byte-level drift surfaces there, while this op tells the
/// executor what the scripting channel must select.
fn plan_wallpaper(desktop: &str, plugin: &str, image: &str) -> RestoreOp {
    RestoreOp::RestoreWallpaper {
        desktop: desktop.to_owned(),
        plugin: plugin.to_owned(),
        image: image.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{DriftKind, RestoreOp, plan_restore};
    use crate::fs::sandbox::SafePath;
    use crate::fs::{FileSystem, RealFileSystem};
    use crate::snapshots::capture::{CaptureEntry, CaptureSpec, capture_snapshot};

    fn spec(snapshot_dir: SafePath, entries: Vec<CaptureEntry>) -> CaptureSpec {
        CaptureSpec {
            snapshot_id: "01900000-0000-7000-8000-000000000001".to_owned(),
            transaction_id: "01900000-0000-7000-8000-000000000002".to_owned(),
            created_at: "2026-10-21T00:00:00Z".to_owned(),
            app_version: "0.1.0".to_owned(),
            plasma_version: None,
            session: "wayland".to_owned(),
            snapshot_dir,
            entries,
        }
    }

    fn safe(root: &std::path::Path, name: &str) -> SafePath {
        SafePath::new(root.join(name)).expect("safe path")
    }

    #[test]
    fn unchanged_targets_plan_as_noops_without_warnings() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let source = safe(directory.path(), "kdeglobals");
        fs.write_file_atomic(&source, b"[General]\nColorScheme=BreezeDark\n", 0o644)
            .expect("write");
        let snapshot_dir = safe(directory.path(), "snap");
        capture_snapshot(
            &fs,
            &spec(
                snapshot_dir.clone(),
                vec![
                    CaptureEntry::ConfigKey {
                        file: "kdeglobals".to_owned(),
                        path: source.clone(),
                        group: "General".to_owned(),
                        key: "ColorScheme".to_owned(),
                    },
                    CaptureEntry::ConfigFile {
                        path: source.clone(),
                    },
                ],
            ),
        )
        .expect("capture");

        let resolve = |file: &str| (file == "kdeglobals").then(|| source.clone());
        let plan = plan_restore(&fs, &snapshot_dir, &resolve).expect("plan");
        assert!(plan.warnings.is_empty());
        // The file restore is a noop (content agrees); the key entry still records
        // its exact restore for the conditional rollback path.
        assert!(
            plan.ops
                .iter()
                .any(|op| matches!(op, RestoreOp::Noop { .. }))
        );
        assert!(
            plan.ops
                .iter()
                .any(|op| matches!(op, RestoreOp::RestoreKey { value: Some(_), .. }))
        );
    }

    #[test]
    fn changed_files_warn_and_absent_paths_remove() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let source = safe(directory.path(), "kdeglobals");
        fs.write_file_atomic(&source, b"[General]\n", 0o644)
            .expect("write");
        let created = safe(directory.path(), "created");
        let snapshot_dir = safe(directory.path(), "snap");
        capture_snapshot(
            &fs,
            &spec(
                snapshot_dir.clone(),
                vec![
                    CaptureEntry::ConfigFile {
                        path: source.clone(),
                    },
                    CaptureEntry::ConfigFile {
                        path: created.clone(),
                    },
                ],
            ),
        )
        .expect("capture");
        fs.write_file_atomic(&source, b"[General]\nChanged=yes\n", 0o644)
            .expect("drift");
        fs.write_file_atomic(&created, b"new", 0o644)
            .expect("created");

        let resolve = |_: &str| None;
        let plan = plan_restore(&fs, &snapshot_dir, &resolve).expect("plan");
        assert_eq!(plan.warnings.len(), 2);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.kind == DriftKind::FileChanged)
        );
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.kind == DriftKind::CreatedSince)
        );
        assert!(
            plan.ops
                .iter()
                .any(|op| matches!(op, RestoreOp::RestoreFile { .. }))
        );
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            RestoreOp::RemovePath {
                live_existed: true,
                ..
            }
        )));
    }

    #[test]
    fn damaged_snapshots_are_refused_not_planned() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let source = safe(directory.path(), "kdeglobals");
        fs.write_file_atomic(&source, b"data", 0o644)
            .expect("write");
        let snapshot_dir = safe(directory.path(), "snap");
        capture_snapshot(
            &fs,
            &spec(
                snapshot_dir.clone(),
                vec![CaptureEntry::ConfigFile { path: source }],
            ),
        )
        .expect("capture");

        let manifest_path = snapshot_dir
            .join_relative("snapshot.json")
            .expect("manifest");
        fs.write_file_atomic(&manifest_path, b"{broken", 0o600)
            .expect("damage");
        let resolve = |_: &str| None;
        let error = plan_restore(&fs, &snapshot_dir, &resolve).expect_err("damaged");
        assert_eq!(error.code, crate::errors::ErrorCode::SnapshotCorrupt);
    }

    #[test]
    fn directory_trees_restore_with_their_files() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let root = safe(directory.path(), "icons");
        let file = root.join_relative("index.theme").expect("file");
        fs.create_dir_all(&root, 0o755).expect("mkdir");
        fs.write_file_atomic(&file, b"[Icon Theme]\n", 0o644)
            .expect("write");
        let snapshot_dir = safe(directory.path(), "snap");
        capture_snapshot(
            &fs,
            &spec(
                snapshot_dir.clone(),
                vec![CaptureEntry::AssetDir { path: root.clone() }],
            ),
        )
        .expect("capture");
        fs.write_file_atomic(&file, b"drifted", 0o644)
            .expect("drift");
        let resolve = |_: &str| None;
        let plan = plan_restore(&fs, &snapshot_dir, &resolve).expect("plan");
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.kind == DriftKind::TreeChanged)
        );
        let [
            RestoreOp::RestoreDir {
                files, tree_hash, ..
            },
        ] = plan.ops.as_slice()
        else {
            panic!("expected one grouped directory restore");
        };
        assert_eq!(files.len(), 1);
        assert!(!tree_hash.is_empty());
    }
}
