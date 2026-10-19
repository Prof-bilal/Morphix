//! Structured failure reports: what ran, what was reversed, and whether the desktop is
//! actually back to its previous state.
//!
//! These types exist so a failure can never be reported as a bare error string. The
//! caller always learns which operations ran, whether reversal was *verified*, and
//! whether manual recovery is required (ARCHITECTURE.md §12, TRANSACTIONS.md §8, §12).
//!
//! The central rule of the product lives here: `Succeeded` cannot be constructed without
//! proof of verification, so no code path can claim "your previous state was restored"
//! while guessing.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::code::ErrorCode;

/// Proof that a reversal was verified against the snapshot.
///
/// The type has no public constructor: the only way to obtain one is
/// [`Verified::after_verification`] with a passing verification result. Serialises as the
/// boolean `true`, and refuses to deserialise anything else, so a payload that claims
/// success without proof is rejected at the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified(());

impl Verified {
    /// Returns proof only when verification actually passed.
    #[must_use]
    pub const fn after_verification(passed: bool) -> Option<Self> {
        if passed { Some(Self(())) } else { None }
    }

    /// Always `true`: the type cannot exist otherwise.
    #[must_use]
    pub const fn is_verified(self) -> bool {
        true
    }
}

impl Serialize for Verified {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for Verified {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if bool::deserialize(deserializer)? {
            Ok(Self(()))
        } else {
            Err(de::Error::custom(
                "`succeeded` requires `verified: true`; an unverified reversal is `failed`",
            ))
        }
    }
}

/// Journal state of one operation.
///
/// The string forms match the `transaction_operations.state` constraint in
/// [DATABASE.md] exactly; a drift test enforces it.
///
/// [DATABASE.md]: ../../../../../DATABASE.md
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Journaled, not started.
    Pending,
    /// Journaled as started, outcome unknown until read back.
    Applying,
    /// Applied; verification has not run yet.
    Applied,
    /// Verification in progress.
    Verifying,
    /// Applied and verified against the plan.
    Verified,
    /// Reversal in progress.
    Reversing,
    /// Reversed and restoration verified.
    Reverted,
    /// The operation itself failed.
    Failed,
    /// Deliberately not attempted (for example, an earlier failure made it moot).
    Skipped,
}

/// Outcome of a single operation, as shown in the UI and written to history.
///
/// `target` is a human-readable summary for display. It never contains file contents, and
/// it is not a path the UI may act on (UI_SPEC U-02).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationOutcome {
    /// Zero-based position in the plan.
    pub index: usize,
    /// Capability that owns the operation, for example `panel.properties`.
    pub capability: String,
    /// Human-readable description of what was touched, in user terms.
    pub target: String,
    /// Journal state at the time the report was built.
    pub state: OperationState,
    /// Whether the apply (or, for `Reverted`, the restoration) was verified.
    pub verified: bool,
    /// Failure code, when this operation failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<ErrorCode>,
}

/// Result of a reversal attempt.
///
/// There is no `Failed { verified: false }`: an unverified reversal is `Failed`, and the
/// only representation of success carries [`Verified`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RollbackOutcome {
    /// No mutation had happened yet, so there was nothing to reverse.
    NotNeeded,
    /// Every completed operation was reversed and restoration was verified.
    Succeeded {
        /// Proof of verification; always `true` on the wire.
        verified: Verified,
    },
    /// Reversal failed, or could not be verified.
    Failed {
        /// Why the reversal failed.
        code: ErrorCode,
        /// What the user needs to know, in Morphix's own words.
        message: String,
    },
}

impl RollbackOutcome {
    /// Whether the previous state is known to have been restored.
    ///
    /// Only `Succeeded` may say "restored" (UI_SPEC U-04).
    #[must_use]
    pub const fn restoration_verified(&self) -> bool {
        matches!(self, Self::Succeeded { .. })
    }
}

/// Manual recovery information for a transaction that could not be repaired
/// automatically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryReport {
    /// Transaction that requires attention.
    pub transaction_id: String,
    /// Resources whose state is unknown or partially changed, in user terms.
    pub affected_resources: Vec<String>,
    /// Snapshot objects the user or a later repair can inspect.
    pub snapshot_path: String,
    /// What the user should do next.
    pub instructions: String,
}

/// Structured result of a failed apply or rollback.
///
/// This is the only shape in which a mutation may fail, so the UI never has to infer from
/// an error code whether the desktop was restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureReport {
    /// Transaction the failure belongs to.
    ///
    /// A plain string until the `TransactionId` newtype lands in P2-02; the field name is
    /// the contract, the representation is not.
    pub transaction_id: String,
    /// The code for the overall failure.
    pub code: ErrorCode,
    /// Index of the operation that failed, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_operation: Option<usize>,
    /// Every operation the engine touched, in plan order.
    pub completed: Vec<OperationOutcome>,
    /// Whether the previous state was restored.
    pub rollback: RollbackOutcome,
    /// Present when automatic recovery was impossible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RecoveryReport>,
}

impl FailureReport {
    /// True when the report may tell the user their previous state was restored.
    #[must_use]
    pub const fn restoration_verified(&self) -> bool {
        self.rollback.restoration_verified()
    }
}

#[cfg(test)]
mod tests {
    // Tests fail loudly: `expect` with a message is allowed here even though the library
    // target denies it (CONTRIBUTING.md).
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        FailureReport, OperationOutcome, OperationState, RecoveryReport, RollbackOutcome, Verified,
    };
    use crate::errors::ErrorCode;
    use std::collections::BTreeSet;

    const DATABASE: &str = include_str!("../../../../DATABASE.md");

    fn succeeded() -> RollbackOutcome {
        RollbackOutcome::Succeeded {
            verified: Verified::after_verification(true).expect("verification passed"),
        }
    }

    fn failed() -> RollbackOutcome {
        RollbackOutcome::Failed {
            code: ErrorCode::RollbackFailed,
            message: "reversal could not be verified".to_owned(),
        }
    }

    #[test]
    fn success_requires_proof_of_verification() {
        assert!(Verified::after_verification(true).is_some());
        assert!(
            Verified::after_verification(false).is_none(),
            "a failed verification must not produce proof"
        );

        let json = serde_json::to_string(&succeeded()).expect("succeeds serialises");
        assert_eq!(json, r#"{"outcome":"succeeded","verified":true}"#);

        let parsed: RollbackOutcome =
            serde_json::from_str(r#"{"outcome":"succeeded","verified":true}"#)
                .expect("verified success deserialises");
        assert!(parsed.restoration_verified());

        for denied in [
            r#"{"outcome":"succeeded","verified":false}"#,
            r#"{"outcome":"succeeded"}"#,
        ] {
            assert!(
                serde_json::from_str::<RollbackOutcome>(denied).is_err(),
                "{denied} claims success without proof and must be rejected"
            );
        }
    }

    #[test]
    fn only_succeeded_may_claim_restoration() {
        assert!(!RollbackOutcome::NotNeeded.restoration_verified());
        assert!(succeeded().restoration_verified());
        assert!(!failed().restoration_verified());

        let report = FailureReport {
            transaction_id: "tx-1".to_owned(),
            code: ErrorCode::ApplyFailed,
            failed_operation: Some(2),
            completed: Vec::new(),
            rollback: failed(),
            recovery: Some(RecoveryReport {
                transaction_id: "tx-1".to_owned(),
                affected_resources: vec!["Color scheme".to_owned()],
                snapshot_path: "/tmp/morphix/snapshots/tx-1".to_owned(),
                instructions: "Open Diagnostics for the snapshot path.".to_owned(),
            }),
        };
        assert!(
            !report.restoration_verified(),
            "a failed reversal may never claim success"
        );
    }

    #[test]
    fn a_report_round_trips_and_omits_absent_optionals() {
        let report = FailureReport {
            transaction_id: "tx-2".to_owned(),
            code: ErrorCode::ApplyFailed,
            failed_operation: Some(0),
            completed: vec![OperationOutcome {
                index: 0,
                capability: "appearance.color_scheme".to_owned(),
                target: "Color scheme".to_owned(),
                state: OperationState::Failed,
                verified: false,
                code: Some(ErrorCode::IoPermission),
            }],
            rollback: succeeded(),
            recovery: None,
        };

        let json = serde_json::to_string(&report).expect("report serialises");
        assert!(
            !json.contains("recovery"),
            "absent recovery must not be serialised: {json}"
        );
        assert!(json.contains("\"outcome\":\"succeeded\""));

        let parsed: FailureReport = serde_json::from_str(&json).expect("report deserialises");
        assert_eq!(parsed, report);
    }

    #[test]
    fn operation_state_strings_match_the_database_constraint() {
        let marker = DATABASE
            .find("CREATE TABLE transaction_operations")
            .expect("transaction_operations table is documented");
        let rest = &DATABASE[marker..];
        let check_start = rest
            .find("CHECK (state IN (")
            .expect("state constraint is documented");
        let rest = &rest[check_start..];
        let end = rest.find("))").expect("state constraint is terminated");
        let documented: BTreeSet<String> = rest[..end]
            .split('\'')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect();

        let implemented: BTreeSet<String> = [
            OperationState::Pending,
            OperationState::Applying,
            OperationState::Applied,
            OperationState::Verifying,
            OperationState::Verified,
            OperationState::Reversing,
            OperationState::Reverted,
            OperationState::Failed,
            OperationState::Skipped,
        ]
        .iter()
        .map(|state| serde_json::to_string(state).expect("state serialises"))
        .map(|json| json.trim_matches('"').to_owned())
        .collect();

        assert_eq!(
            implemented, documented,
            "operation states and DATABASE.md's CHECK list must agree"
        );
    }
}
