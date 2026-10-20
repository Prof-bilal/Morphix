//! Validation report types shared by the parser, integrity checks, and the validator.
//!
//! A validation run never stops at the first semantic problem: up to
//! [`MAX_VALIDATION_ISSUES`] issues are collected so the UI can show field-level errors
//! (PACK_SPEC.md §8). Structural parse failures are the exception: without a typed
//! manifest there is nothing further to check.

use serde::{Deserialize, Serialize};

pub use crate::core::resource::IntegritySource;

use crate::core::capability::Capability;
use crate::errors::ErrorCode;
use crate::fs::tree::TreeSnapshot;

use super::manifest::Manifest;

/// Maximum issues returned for one pack (PACK_SPEC.md §8).
pub const MAX_VALIDATION_ISSUES: usize = 100;

/// One field-level validation problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    /// Stable error code.
    pub code: ErrorCode,
    /// User-facing explanation. Never contains file contents.
    pub message: String,
    /// Pack-relative path or field path, when the issue has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Manifest field, when the issue is attached to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl ValidationIssue {
    /// Builds an issue without a path or field.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            path: None,
            field: None,
        }
    }

    /// Attaches a manifest field name.
    #[must_use]
    pub fn at_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    /// Attaches a pack-relative path.
    #[must_use]
    pub fn at_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}

/// One declared change derived from the manifest, before planning.
///
/// The detail view uses this list; Phase 3 turns the same declarations into concrete
/// operations. `kind` is a stable snake-case operation class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredChange {
    /// Capability that owns the change.
    pub capability: Capability,
    /// Operation class (`install_asset`, `write_config`, …).
    pub kind: String,
    /// Human-readable target, without a host path.
    pub target: String,
}

impl DeclaredChange {
    /// Builds a declared change.
    #[must_use]
    pub fn new(capability: Capability, kind: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            capability,
            kind: kind.into(),
            target: target.into(),
        }
    }
}

/// The result of one validation run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationOutcome {
    /// The typed manifest, available only when structural parsing succeeded.
    pub manifest: Option<Manifest>,
    /// Canonical manifest hash, when parsing succeeded.
    pub manifest_hash: Option<String>,
    /// Deterministic hash of the entire pack tree.
    pub content_hash: String,
    /// Inspected tree, used by the installer for its post-copy comparison.
    pub tree: TreeSnapshot,
    /// Size of `pack.toml` in bytes.
    pub manifest_bytes: u64,
    /// Collected issues, capped at [`MAX_VALIDATION_ISSUES`].
    pub issues: Vec<ValidationIssue>,
    /// Declared changes, in manifest order.
    pub declared_changes: Vec<DeclaredChange>,
    /// Integrity provenance.
    pub integrity_source: IntegritySource,
}

impl ValidationOutcome {
    /// Whether the pack may be installed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty() && self.manifest.is_some()
    }

    /// Whether the tree was inspected at all.
    #[must_use]
    pub fn checked_tree(&self) -> bool {
        !self.content_hash.is_empty()
    }
}
