//! Morphix domain engine.
//!
//! This crate holds all domain logic: detection, pack handling, planning, snapshots, and
//! the transaction engine. It must not depend on Tauri, a GUI toolkit, or a display
//! server, so that every rule in the design documents stays testable headlessly
//! (ARCHITECTURE.md §3, §16).
//!
//! Module ownership and the allowed dependency edges are described in ARCHITECTURE.md §4
//! and §5, and are enforced by `tests/architecture.rs` (P0-05).
//!
//! # Status
//!
//! Phases 0–4 are implemented: errors, IDs, capabilities, allowlists, platform
//! detection, filesystem safety, pack parsing/validation/store, SQLite storage, the
//! read-only change planner, and the snapshot engine (capture, object store,
//! manifest, verification, GC, persistence, restore planning, failure tests, and
//! the mutation lock). `transactions` is still a module declaration, and the
//! `desktop::kde` adapter still ends at presence probing: live reads and the full
//! `DesktopAdapter` arrive in Phases 5–6. A module's nested files are created when
//! its first task lands, not in advance (TASKS.md).

// Library code must not panic: every failure is a typed error. These rules apply to the
// library target only, so test targets can still `unwrap` a broken assertion
// (AGENTS.md §11, CONTRIBUTING.md).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

pub mod core;
pub mod desktop;
pub mod errors;
pub mod fs;
pub mod logging;
pub mod packs;
pub mod permissions;
pub mod planning;
pub mod platform;
pub mod snapshots;
pub mod storage;
pub mod system;
pub mod transactions;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::core::resource::{PackSource, PackStatus};
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::packs::catalog::{self, Candidate, PackDetail, PackSummary};
use crate::packs::manifest::Manifest;
use crate::packs::parser::{MANIFEST_FILE, parse_manifest};
use crate::packs::report::{DeclaredChange, ValidationOutcome};
use crate::platform::paths::MorphixPaths;
use crate::storage::db::{Storage, now_rfc3339};
use crate::storage::repo::packs::InstalledPack;

/// The application facade: composes detection, packs, and storage for the IPC layer.
///
/// The facade is a leaf. Domain modules never import it or each other's facade
/// (ARCHITECTURE.md §5). Catalog reads and installs are serialized through one `Storage`
/// connection; install validates and copies before registering anything.
pub struct MorphixService {
    paths: MorphixPaths,
    storage: Mutex<Storage>,
    bundled_root: Option<PathBuf>,
}

impl std::fmt::Debug for MorphixService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MorphixService")
            .field("paths", &self.paths)
            .field("bundled_root", &self.bundled_root)
            .finish_non_exhaustive()
    }
}

impl MorphixService {
    /// Opens the persistent database and prepares the catalog.
    ///
    /// # Errors
    ///
    /// Propagates database open/migration failures.
    pub fn open(paths: MorphixPaths) -> Result<Self> {
        Self::open_with_bundled_root(paths, None)
    }

    /// Opens the service with an optional bundled-pack directory.
    ///
    /// # Errors
    ///
    /// Propagates database open/migration failures.
    pub fn open_with_bundled_root(
        paths: MorphixPaths,
        bundled_root: Option<PathBuf>,
    ) -> Result<Self> {
        let storage = Storage::open(&paths.database_path())?;
        Ok(Self {
            paths,
            storage: Mutex::new(storage),
            bundled_root,
        })
    }

    /// Builds a service around an already-open storage handle (tests and embedding).
    #[must_use]
    pub fn from_parts(
        paths: MorphixPaths,
        storage: Storage,
        bundled_root: Option<PathBuf>,
    ) -> Self {
        Self {
            paths,
            storage: Mutex::new(storage),
            bundled_root,
        }
    }

    /// The resolved Morphix paths.
    #[must_use]
    pub fn paths(&self) -> &MorphixPaths {
        &self.paths
    }

    /// Lists bundled, installed, and invalid packs.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the pack index cannot be read or the bundled root is not
    /// a readable directory.
    pub fn list_packs(&self) -> Result<Vec<PackSummary>> {
        let candidates = self.candidates()?;
        catalog::list(&candidates)
    }

    /// Loads one pack detail.
    ///
    /// # Errors
    ///
    /// Returns `E_INVALID_ID` when no available pack has that ID, and a typed error when
    /// the pack index or bundled root cannot be read.
    pub fn get_pack_detail(&self, pack_id: &str) -> Result<PackDetail> {
        let candidates = self.candidates()?;
        catalog::detail(&candidates, pack_id)?.ok_or_else(|| {
            MorphixError::new(ErrorCode::InvalidId, "no available pack has this ID")
                .with_context("pack_id", pack_id)
        })
    }

    /// Validates and installs a pack from a user-chosen local directory.
    ///
    /// Refuses to run as root, validates before writing, copies through a staging
    /// directory, verifies the copied tree hash, and only then registers the pack. A
    /// rejected install leaves the pack store and any desktop target untouched; the
    /// rejection is recorded as a `pack_install_rejected` history event.
    ///
    /// # Errors
    ///
    /// Returns the validation error with all issues in `detail`, `E_PACK_ALREADY_INSTALLED`
    /// for the same version, `E_PACK_ID_CONFLICT` for a different version, and
    /// `E_DEPENDENCY_CYCLE` for a dependency cycle.
    pub fn install_local_pack(&self, directory: &Path) -> Result<PackSummary> {
        platform::session::probe()?.require_non_root()?;

        let source = absolute_source(directory)?;
        let outcome = packs::validator::validate_directory(&source)?;
        if !outcome.is_valid() {
            self.record_rejection(&outcome, directory);
            return Err(validation_error(&outcome));
        }
        let Some(manifest) = outcome.manifest.as_ref() else {
            self.record_rejection(&outcome, directory);
            return Err(validation_error(&outcome));
        };
        let pack_id = manifest.pack.id.clone();
        let version = manifest.pack.version.to_string();

        {
            let storage = self.lock_storage()?;
            if let Some(existing) = storage.get_installed_pack(pack_id.as_str())? {
                return Err(if existing.version == version {
                    MorphixError::new(
                        ErrorCode::PackAlreadyInstalled,
                        "this pack version is already installed",
                    )
                    .with_context("pack_id", pack_id.as_str())
                    .with_context("version", version)
                } else {
                    MorphixError::new(
                        ErrorCode::PackIdConflict,
                        "a different version of this pack is already installed; V0.1 does \
                         not upgrade packs",
                    )
                    .with_context("pack_id", pack_id.as_str())
                    .with_context("installed_version", existing.version)
                    .with_context("requested_version", version)
                });
            }
        }

        if self.dependency_cycle(manifest)? {
            self.record_rejection(&outcome, directory);
            return Err(MorphixError::new(
                ErrorCode::DependencyCycle,
                "installing this pack would create a dependency cycle",
            )
            .with_context("pack_id", pack_id.as_str()));
        }

        let packs_dir = self.paths.packs_dir();
        let snapshot =
            packs::store::install_tree(&source, &packs_dir, &pack_id, &outcome.content_hash)?;
        let store_path = packs::store::pack_directory(&packs_dir, &pack_id);
        let record = InstalledPack {
            pack_id: pack_id.to_string(),
            version: version.clone(),
            manifest_hash: outcome.manifest_hash.clone().unwrap_or_default(),
            content_hash: snapshot.content_hash(),
            source: PackSource::Local,
            store_path: store_path.display().to_string(),
            integrity_source: outcome.integrity_source,
            status: PackStatus::Valid,
            installed_at: now_rfc3339(),
            last_validated_at: Some(now_rfc3339()),
        };

        if let Err(error) = self.lock_storage()?.insert_pack(&record) {
            if let Err(cleanup) = packs::store::remove_installed(&packs_dir, &pack_id) {
                return Err(error.with_context(
                    "cleanup_error",
                    format!("{}: {}", cleanup.code.as_str(), cleanup.message),
                ));
            }
            return Err(error);
        }

        Ok(catalog::summarize(
            &Candidate {
                root: store_path,
                source: PackSource::Local,
                applied: false,
                fallback_id: pack_id.to_string(),
            },
            &outcome,
        ))
    }

    /// Builds a change plan for an installed pack (P3-11).
    ///
    /// Until Phase 6 injects the real KDE read path, this answers
    /// `E_ADAPTER_UNAVAILABLE` rather than returning an empty or optimistic plan
    /// ([ADR-024](DECISIONS.md#adr-024--acceptance-criteria-follow-the-phase-that-supplies-their-dependency)).
    /// The planner itself (`planning::planner::plan_pack`) is fully implemented and
    /// covered at planner level with fixture packs and injected pre-state.
    ///
    /// # Errors
    ///
    /// Returns `E_ADAPTER_UNAVAILABLE` on every path in Phase 3, and `E_INVALID_ID`
    /// when no available pack has the requested ID.
    pub fn create_pack_plan(&self, pack_id: &str) -> Result<crate::planning::plan::ChangePlan> {
        self.get_pack_detail(pack_id)?;
        Err(MorphixError::new(
            ErrorCode::AdapterUnavailable,
            "planning from live KDE state arrives with the Phase 6 adapter reads; no plan \
             was created and nothing was written",
        )
        .with_context("pack_id", pack_id)
        .with_context("phase", "Phase 6"))
    }

    /// Builds a plan restoring a snapshot (stub until Phase 5).
    ///
    /// # Errors
    ///
    /// Always returns `E_UNSUPPORTED_RESOURCE` in Phase 4: the engine-level plan
    /// exists (`snapshots::restore::plan_restore`, with drift warnings), but
    /// mapping it onto an applicable `ChangePlan` needs the Phase 5 apply path —
    /// `OperationAction` has no file-restore, tree-restore, or remove-path
    /// operation yet, and inventing one here would bypass the transaction engine.
    /// No plan is created and nothing is written.
    pub fn create_snapshot_restore_plan(
        &self,
        snapshot_id: &str,
    ) -> Result<crate::planning::plan::ChangePlan> {
        Err(MorphixError::new(
            ErrorCode::UnsupportedResource,
            "snapshot restore execution arrives with the Phase 5 transaction engine; no \
             plan was created and nothing was written",
        )
        .with_context("snapshot_id", snapshot_id))
    }

    /// Discards a persisted plan so it can never apply (P3-09).
    ///
    /// Discard is idempotent: discarding a missing, expired, or already-discarded plan
    /// succeeds without writing desktop state. Expired plans are collected first.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the plan store cannot be updated.
    pub fn discard_plan(&self, plan_id: &str) -> Result<()> {
        let mut storage = self.lock_storage()?;
        let _ = storage.delete_expired_plans(&now_rfc3339())?;
        let _ = storage.discard_plan(plan_id)?;
        Ok(())
    }

    fn lock_storage(&self) -> Result<MutexGuard<'_, Storage>> {
        self.storage.lock().map_err(|_| {
            MorphixError::new(
                ErrorCode::Internal,
                "the database mutex was poisoned by a previous panic",
            )
        })
    }

    fn candidates(&self) -> Result<Vec<Candidate>> {
        let (installed, installations) = {
            let storage = self.lock_storage()?;
            (
                storage.list_installed_packs()?,
                storage.pack_installations()?,
            )
        };

        let mut candidates = Vec::new();
        for pack in installed {
            let applied = installations
                .get(&pack.pack_id)
                .is_some_and(|installation| installation.applied);
            candidates.push(Candidate {
                root: PathBuf::from(&pack.store_path),
                source: pack.source,
                applied,
                fallback_id: pack.pack_id,
            });
        }

        if let Some(root) = &self.bundled_root {
            for directory in catalog::scan_directory(root)? {
                let fallback_id = directory
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                candidates.push(Candidate {
                    root: directory,
                    source: PackSource::Bundled,
                    applied: false,
                    fallback_id,
                });
            }
        }
        Ok(candidates)
    }

    /// Records a rejected install without touching the pack store.
    ///
    /// A history failure is logged and never masks why the pack was rejected.
    fn record_rejection(&self, outcome: &ValidationOutcome, directory: &Path) {
        let details = serde_json::json!({
            "codes": outcome.issues.iter().map(|issue| issue.code.as_str()).collect::<Vec<_>>(),
            "issueCount": outcome.issues.len(),
        });
        let summary = format!(
            "Rejected local pack from {}",
            directory.file_name().map_or_else(
                || directory.display().to_string(),
                |name| name.to_string_lossy().into_owned()
            )
        );
        let pack_id = outcome
            .manifest
            .as_ref()
            .map(|manifest| manifest.pack.id.to_string());
        if let Ok(mut storage) = self.storage.lock()
            && let Err(error) =
                storage.record_rejected_install(pack_id.as_deref(), &summary, &details)
        {
            tracing::warn!(
                error_code = error.code.as_str(),
                "failed to record a rejected pack install"
            );
        }
    }

    fn dependency_cycle(&self, manifest: &Manifest) -> Result<bool> {
        let Some(dependencies) = &manifest.dependencies else {
            return Ok(false);
        };
        if dependencies.packs.is_empty() {
            return Ok(false);
        }

        let installed = {
            let storage = self.lock_storage()?;
            storage.list_installed_packs()?
        };
        let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for pack in &installed {
            let manifest_path = Path::new(&pack.store_path).join(MANIFEST_FILE);
            let edges = match std::fs::read(&manifest_path)
                .ok()
                .and_then(|bytes| parse_manifest(&bytes).ok())
            {
                Some(parsed) => parsed
                    .manifest
                    .dependencies
                    .map(|dependencies| {
                        dependencies
                            .packs
                            .into_iter()
                            .map(|dependency| dependency.id)
                            .collect()
                    })
                    .unwrap_or_default(),
                None => {
                    // A broken installed pack cannot contribute reliable edges. It also
                    // cannot be planned, so this does not hide a cycle for a valid pack.
                    tracing::warn!(
                        pack_id = pack.pack_id.as_str(),
                        "installed pack manifest could not be parsed during cycle check"
                    );
                    Vec::new()
                }
            };
            graph.insert(pack.pack_id.clone(), edges);
        }
        let new_id = manifest.pack.id.to_string();
        graph.insert(
            new_id.clone(),
            dependencies
                .packs
                .iter()
                .map(|dependency| dependency.id.clone())
                .collect(),
        );

        let mut visited = BTreeSet::new();
        Ok(reaches(&graph, &new_id, &new_id, &mut visited, true))
    }
}

fn reaches(
    graph: &BTreeMap<String, Vec<String>>,
    current: &str,
    target: &str,
    visited: &mut BTreeSet<String>,
    root: bool,
) -> bool {
    if !root && current == target {
        return true;
    }
    if !visited.insert(current.to_owned()) {
        return false;
    }
    graph
        .get(current)
        .into_iter()
        .flatten()
        .any(|next| reaches(graph, next, target, visited, false))
}

fn absolute_source(directory: &Path) -> Result<PathBuf> {
    let metadata = std::fs::symlink_metadata(directory)
        .map_err(|error| fs::sandbox::io_error(directory, &error))?;
    if metadata.file_type().is_symlink() {
        return Err(MorphixError::new(
            ErrorCode::SymlinkRejected,
            "the selected pack directory is a symbolic link",
        )
        .with_context("path", directory.display().to_string()));
    }
    if !metadata.is_dir() {
        return Err(MorphixError::new(
            ErrorCode::FileTypeRejected,
            "the selected path is not a directory",
        )
        .with_context("path", directory.display().to_string()));
    }
    std::fs::canonicalize(directory).map_err(|error| fs::sandbox::io_error(directory, &error))
}

fn validation_error(outcome: &ValidationOutcome) -> MorphixError {
    let code = outcome
        .issues
        .first()
        .map_or(ErrorCode::Internal, |issue| issue.code);
    let codes: Vec<&str> = outcome
        .issues
        .iter()
        .map(|issue| issue.code.as_str())
        .collect();
    let detail = serde_json::json!({
        "issues": outcome.issues,
        "codes": codes,
    });
    MorphixError::new(code, "the pack failed validation; nothing was installed").with_detail(detail)
}

/// Compile-time assertion that the service can be shared with Tauri state.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MorphixService>();
};

/// Declared changes for a pack, re-exported for thin commands.
#[must_use]
pub fn declared_changes(detail: &PackDetail) -> &[DeclaredChange] {
    &detail.declared_changes
}

/// Crate version, as declared in `Cargo.toml`.
///
/// Reported through `get_app_info` (P0-07) and shown in Diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn crate_version_is_semver() {
        assert!(
            semver::Version::parse(VERSION).is_ok(),
            "CARGO_PKG_VERSION is not a valid semver version: {VERSION}"
        );
    }
}
