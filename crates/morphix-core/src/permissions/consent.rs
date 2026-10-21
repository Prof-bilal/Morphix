//! Consent token bound to a plan hash (P3-11 shape, enforced in Phase 5).
//!
//! **Ownership.** Proof that the user agreed to the exact plan being applied. This
//! module defines the token shape and its hash binding; the transaction engine checks
//! expiry, staleness, and the mutation lock at apply time (TRANSACTIONS.md 7).
//!
//! **Allowed dependencies:** `errors`, `core`.
//! **Must never depend on:** `fs`, `desktop`, `transactions`, `packs`.
//!
//! `permissions` may not depend on `planning`, so the token carries the plan hash as a
//! plain string rather than importing [`PlanHash`](crate::planning::plan::PlanHash).

use serde::{Deserialize, Serialize};

use crate::core::capability::Capability;
use crate::errors::{ErrorCode, MorphixError, Result};

/// Proof of explicit user consent for one plan hash.
///
/// Issued when the user confirms the rendered plan; verified before any mutation.
/// The token is bound to the exact [`PlanHash`](crate::planning::plan::PlanHash) string:
/// planning again after any state change yields a different hash and the old token no
/// longer applies (`E_PLAN_STALE` at apply time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentToken {
    /// The plan hash the user reviewed, hex-encoded.
    pub plan_hash: String,
    /// Capabilities the user granted, in documentation order.
    pub capabilities: Vec<Capability>,
    /// RFC 3339 issuance time.
    pub issued_at: String,
}

impl ConsentToken {
    /// Issues a token for a reviewed plan hash.
    #[must_use]
    pub fn issue(
        plan_hash: impl Into<String>,
        capabilities: Vec<Capability>,
        issued_at: impl Into<String>,
    ) -> Self {
        let mut granted = capabilities;
        granted.sort();
        granted.dedup();
        Self {
            plan_hash: plan_hash.into(),
            capabilities: granted,
            issued_at: issued_at.into(),
        }
    }

    /// Verifies the token is bound to `plan_hash`.
    ///
    /// # Errors
    ///
    /// Returns `E_CONSENT_REQUIRED` when the hashes differ. Expiry and precondition
    /// re-checks belong to the apply path (Phase 5), not to this binding check.
    pub fn verify_binding(&self, plan_hash: &str) -> Result<()> {
        if self.plan_hash == plan_hash {
            Ok(())
        } else {
            Err(MorphixError::new(
                ErrorCode::ConsentRequired,
                "consent is bound to a different plan; review the current plan before applying",
            )
            .with_context("expected_hash", plan_hash))
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::ConsentToken;
    use crate::core::capability::Capability;
    use crate::errors::ErrorCode;

    #[test]
    fn binding_matches_only_the_reviewed_hash() {
        let token = ConsentToken::issue(
            "abc123",
            vec![Capability::Wallpaper, Capability::ColorScheme],
            "2026-10-21T00:00:00Z",
        );
        assert_eq!(
            token.capabilities,
            vec![Capability::ColorScheme, Capability::Wallpaper]
        );
        assert!(token.verify_binding("abc123").is_ok());
        let error = token
            .verify_binding("different")
            .expect_err("mismatched hash is refused");
        assert_eq!(error.code, ErrorCode::ConsentRequired);
    }
}
