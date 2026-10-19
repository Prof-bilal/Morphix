//! Error taxonomy and structured failure reports.
//!
//! **Ownership.** The stable error code registry, the crate error type, and the report
//! types that make a failure impossible to describe as a bare string.
//!
//! **Allowed dependencies:** `std`, `serde`, `thiserror`. This module is a leaf: it must
//! never depend on another module in this crate (ARCHITECTURE.md §5).

pub mod code;
pub mod report;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use code::{ErrorCode, UnknownErrorCode};
pub use report::{
    FailureReport, OperationOutcome, OperationState, RecoveryReport, RollbackOutcome, Verified,
};

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, MorphixError>;

/// One piece of diagnostic context.
///
/// Entries are ordered and their keys are stable, so two runs can be diffed. Values are
/// short and safe to log: a config key, a path, a version, a hash. Never file contents,
/// asset bytes, or a value from a key marked sensitive (SECURITY.md T-23).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextEntry {
    /// Short machine-readable key, for example `config_file`.
    pub key: String,
    /// Human-readable value, safe to log.
    pub value: String,
}

/// The crate's error type: a stable code, one sentence of user-facing meaning, optional
/// structured detail, and ordered context.
///
/// `Display` renders as `<CODE>: <message>`, so a log line is greppable by code and no
/// error ever loses its code (AGENTS.md §10). Errors that mutate or fail to mutate state
/// are reported as a [`FailureReport`] instead; this type alone never implies that
/// nothing changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Error)]
#[error("{code}: {message}")]
pub struct MorphixError {
    /// Stable code from the registry.
    pub code: ErrorCode,
    /// What happened, in one sentence, in Morphix's own words.
    pub message: String,
    /// Optional structured payload: field errors, conflicting resources, and similar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
    /// Ordered diagnostic context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<ContextEntry>,
}

impl MorphixError {
    /// Builds an error from a code and a message.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
            context: Vec::new(),
        }
    }

    /// Attaches structured detail, for example per-field validation errors.
    #[must_use]
    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }

    /// Appends a context entry, preserving insertion order.
    #[must_use]
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.push(ContextEntry {
            key: key.into(),
            value: value.into(),
        });
        self
    }
}

/// An error carrying nothing but its code: the message is the code string.
///
/// Used where the code is self-explanatory or the detailed message is supplied by a
/// report, never as a way to hide information the caller needs.
impl From<ErrorCode> for MorphixError {
    fn from(code: ErrorCode) -> Self {
        Self::new(code, code.as_str())
    }
}

#[cfg(test)]
mod tests {
    // Tests fail loudly: `expect` with a message is allowed here even though the library
    // target denies it (CONTRIBUTING.md).
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{ContextEntry, ErrorCode, MorphixError};
    use serde_json::json;

    #[test]
    fn display_is_greppable_by_code() {
        let error = MorphixError::new(ErrorCode::Busy, "another Morphix window is applying a pack");
        assert_eq!(
            error.to_string(),
            "E_BUSY: another Morphix window is applying a pack"
        );
    }

    #[test]
    fn code_only_error_uses_the_code_as_its_message() {
        let error = MorphixError::from(ErrorCode::PlanExpired);
        assert_eq!(error.code, ErrorCode::PlanExpired);
        assert_eq!(error.message, "E_PLAN_EXPIRED");
        assert!(error.detail.is_none());
        assert!(error.context.is_empty());
    }

    #[test]
    fn context_order_is_preserved_and_survives_serialization() {
        let error = MorphixError::new(
            ErrorCode::TargetExists,
            "the target is owned by another pack",
        )
        .with_context("target", "~/.config/kdeglobals [Icons] Theme")
        .with_context("owner", "cupertino")
        .with_detail(json!({ "conflicts": ["cupertino"] }));

        assert_eq!(error.context.len(), 2);
        assert_eq!(error.context[0].key, "target");
        assert_eq!(error.context[1].key, "owner");

        let json = serde_json::to_string(&error).expect("error serialises");
        assert!(json.contains(r#""code":"E_TARGET_EXISTS""#), "{json}");

        let parsed: MorphixError = serde_json::from_str(&json).expect("error deserialises");
        assert_eq!(parsed, error);

        let expected = ContextEntry {
            key: "owner".to_owned(),
            value: "cupertino".to_owned(),
        };
        assert_eq!(parsed.context[1], expected);
    }

    #[test]
    fn empty_detail_and_context_are_omitted_from_the_wire() {
        let error = MorphixError::new(ErrorCode::Internal, "impossible state");
        let json = serde_json::to_string(&error).expect("error serialises");
        assert_eq!(
            json,
            r#"{"code":"E_INTERNAL","message":"impossible state"}"#
        );
    }
}
