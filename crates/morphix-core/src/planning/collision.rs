//! Target collision and ownership checks (P3-06).
//!
//! Two operations in one plan must never resolve to the same resource, and a plan must
//! never overwrite a target owned by another pack or by no pack. The ownership snapshot
//! is passed in by the service facade so `planning` keeps its ban on depending on
//! `storage` (ARCHITECTURE.md 5).

use std::collections::{BTreeMap, BTreeSet};

use super::expand::ExpandedOp;
use super::reads::TargetOwnership;
use crate::errors::{ErrorCode, MorphixError, Result};

/// Refuses duplicate targets within one plan.
///
/// # Errors
///
/// Returns `E_DUPLICATE_TARGET` naming the repeated identity.
pub fn check_duplicate_targets(operations: &[ExpandedOp]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for operation in operations {
        if !seen.insert(operation.target.identity.clone()) {
            return Err(MorphixError::new(
                ErrorCode::DuplicateTarget,
                "two declarations resolve to the same target",
            )
            .with_context("target", &operation.target.identity));
        }
    }
    Ok(())
}

/// Refuses asset targets owned by another pack or present without ownership.
///
/// `ownership` maps engine-relative target paths to their observed state. Config keys,
/// shortcuts, effects, and wallpaper are user state: overwriting them is allowed with
/// an `overwrites_existing_value` warning, not refused.
///
/// # Errors
///
/// Returns `E_TARGET_EXISTS` for an unowned present target or one owned by a different
/// pack at a different version.
pub fn check_asset_ownership(
    operations: &[ExpandedOp],
    ownership: &BTreeMap<String, TargetOwnership>,
    pack_id: &str,
    pack_version: &str,
) -> Result<()> {
    for operation in operations {
        let Some(relative) = operation.target.asset_path.as_deref() else {
            continue;
        };
        let state = ownership
            .get(relative)
            .cloned()
            .unwrap_or(TargetOwnership::Absent);
        match state {
            TargetOwnership::Absent => {}
            TargetOwnership::OwnedBy {
                pack_id: ref owner,
                ref version,
                ..
            } if owner == pack_id && version == pack_version => {}
            TargetOwnership::OwnedBy {
                pack_id: ref owner, ..
            } => {
                return Err(MorphixError::new(
                    ErrorCode::TargetExists,
                    "the target is owned by another pack",
                )
                .with_context("target", &operation.target.identity)
                .with_context("owner", owner));
            }
            TargetOwnership::UnownedPresent { .. } => {
                return Err(MorphixError::new(
                    ErrorCode::TargetExists,
                    "the target exists and is not owned by this pack",
                )
                .with_context("target", &operation.target.identity));
            }
        }
    }
    Ok(())
}

/// Refuses asset targets claimed by a different pack in the installed-asset index.
///
/// The index maps engine-relative target paths to `(pack_id, kind, key)`.
pub fn check_installed_asset_collisions(
    operations: &[ExpandedOp],
    installed: &BTreeMap<String, (String, String, String)>,
    pack_id: &str,
) -> Result<()> {
    for operation in operations {
        let Some(relative) = operation.target.asset_path.as_deref() else {
            continue;
        };
        if let Some((owner, _kind, _key)) = installed.get(relative)
            && owner != pack_id
        {
            return Err(MorphixError::new(
                ErrorCode::TargetExists,
                "the target is already installed by another pack",
            )
            .with_context("target", &operation.target.identity)
            .with_context("owner", owner));
        }
    }
    Ok(())
}
