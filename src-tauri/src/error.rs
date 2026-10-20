//! Core error → IPC error mapping.
//!
//! **Ownership.** The one place that converts a `morphix_core::errors::MorphixError` into
//! the JSON shape the UI renders. The stable code and the structured detail (including
//! validation issues) survive unchanged (ARCHITECTURE.md §12, UI_SPEC.md §8).
//!
//! Rollback and recovery reporting arrive with P5; this type is additive so their fields
//! can be added without changing the command signatures.

use serde::Serialize;
use serde_json::Value;

/// A failed command, serialized to the webview.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    /// Stable core error code (`E_MANIFEST_SYNTAX`, …).
    pub code: String,
    /// One-sentence user-facing explanation.
    pub message: String,
    /// Structured payload such as the full validation issue list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

impl IpcError {
    /// Wraps a worker/join failure that has no core error code.
    #[must_use]
    pub fn internal(command: &str, message: impl Into<String>) -> Self {
        Self {
            code: "E_INTERNAL".to_owned(),
            message: format!("{command}: {}", message.into()),
            detail: None,
        }
    }
}

impl From<morphix_core::errors::MorphixError> for IpcError {
    fn from(error: morphix_core::errors::MorphixError) -> Self {
        Self {
            code: error.code.as_str().to_owned(),
            message: error.message,
            detail: error.detail,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use morphix_core::errors::{ErrorCode, MorphixError};

    use super::IpcError;

    #[test]
    fn a_core_error_keeps_its_code_and_structured_detail() {
        let core = MorphixError::new(ErrorCode::ManifestSyntax, "bad manifest")
            .with_detail(serde_json::json!({ "issues": [{ "code": "E_MANIFEST_SYNTAX" }] }));
        let ipc = IpcError::from(core);
        assert_eq!(ipc.code, "E_MANIFEST_SYNTAX");
        assert_eq!(ipc.message, "bad manifest");
        assert_eq!(
            ipc.detail,
            Some(serde_json::json!({ "issues": [{ "code": "E_MANIFEST_SYNTAX" }] }))
        );
    }

    #[test]
    fn serialization_uses_the_documented_camel_case_shape() {
        let ipc = IpcError {
            code: "E_REQUIRED_FILE_MISSING".to_owned(),
            message: "README.md is missing".to_owned(),
            detail: None,
        };
        let encoded = serde_json::to_value(&ipc).expect("serialize");
        assert_eq!(encoded["code"], "E_REQUIRED_FILE_MISSING");
        assert_eq!(encoded["message"], "README.md is missing");
        assert!(
            encoded.get("detail").is_none(),
            "an empty detail must be omitted"
        );
    }
}
