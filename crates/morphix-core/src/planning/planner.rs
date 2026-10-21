//! Planner orchestration: manifest plus pre-state becomes a plan (P3-10).
//!
//! Pure over [`PlannerReads`]: compatibility, capability, expansion, collision,
//! pre-state capture, warnings, totals, expiry, and the content hash. Any refusal
//! produces a typed error and no plan; the planner never writes.

use std::collections::{BTreeMap, BTreeSet};

use crate::core::capability::Capability;
use crate::desktop::kde::targets::ResourceState;
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::packs::compatibility::PlasmaConstraint;
use crate::packs::manifest::Manifest;

use super::collision::{
    check_asset_ownership, check_duplicate_targets, check_installed_asset_collisions,
};
use super::diff::describe;
use super::expand::{
    ExpandedOp, expand_appearance, expand_effects, expand_settings, expand_shortcuts,
};
use super::plan::{
    ChangePlan, OperationAction, PLAN_TTL_SECONDS, PackRef, PlanHash, PlanId, PlanKind, PlanTotals,
    PlanWarning, PlannedOperation, Precondition, SystemFingerprint, VerificationLevel, WarningKind,
};
use super::reads::PlannerReads;

/// Builds an apply-pack plan from a validated manifest and injected pre-state.
///
/// Pre-state is injected through `reads`; the planner performs no I/O of its own.
/// Timestamps come from `now_secs` (Unix seconds) so tests are deterministic.
///
/// # Errors
///
/// Returns `E_INCOMPATIBLE_PACK` for desktop or Plasma mismatches,
/// `E_CAPABILITY_DENIED` for undeclared use, `E_CAPABILITY_UNUSED` for unused requests,
/// `E_UNSUPPORTED_RESOURCE` for unplannable declarations, `E_TARGET_EXISTS` for owned or
/// foreign targets, `E_DUPLICATE_TARGET` for two declarations hitting one target, and
/// `E_DEPENDENCY_MISSING` for unmet dependencies. Nothing is written on any path.
pub fn plan_pack(reads: &dyn PlannerReads, now_secs: i64) -> Result<ChangePlan> {
    let manifest = reads.manifest()?;
    check_compatibility(manifest, reads)?;
    let mut operations = expand_all(manifest, reads)?;
    normalize_font_capabilities(manifest, &mut operations);
    check_capabilities(manifest, &operations)?;
    check_duplicate_targets(&operations)?;
    check_ownership(manifest, reads, &operations)?;
    apply_wallpaper_gate(reads, &mut operations);
    check_adapter_support(reads, &operations)?;
    let (planned, preconditions, warnings) = capture(reads, manifest, &operations)?;
    finish(manifest, reads, planned, preconditions, warnings, now_secs)
}

/// Expands every manifest section in a stable order.
fn expand_all(manifest: &Manifest, reads: &dyn PlannerReads) -> Result<Vec<ExpandedOp>> {
    let mut operations = expand_appearance(manifest)?;
    operations.extend(expand_settings(manifest)?);
    operations.extend(expand_effects(manifest)?);
    operations.extend(expand_shortcuts(manifest)?);
    let live_panels: Vec<u32> = reads.panels().iter().map(|panel| panel.id).collect();
    operations.extend(super::expand_more::expand_panel(manifest, &live_panels)?);
    let live_desktops: Vec<(Option<String>, String)> = reads
        .virtual_desktops()
        .iter()
        .map(|desktop| (desktop.id.clone(), desktop.name.clone()))
        .collect();
    operations.extend(super::expand_more::expand_desktop_behavior(
        manifest,
        &live_desktops,
    )?);
    if operations.is_empty() {
        return Err(MorphixError::new(
            ErrorCode::UnsupportedResource,
            "the pack declares nothing the planner can express",
        )
        .with_context("pack_id", manifest.pack.id.as_str()));
    }
    Ok(operations)
}

/// Refuses packs for another desktop or an incompatible Plasma version.
///
/// An unknown runtime version is unverified, not incompatible: planning proceeds with a
/// `manual_step` warning (MVP.md 1.1), and the warning is added during capture.
fn check_compatibility(manifest: &Manifest, reads: &dyn PlannerReads) -> Result<()> {
    if manifest.compatibility.desktop != "kde" {
        return Err(MorphixError::new(
            ErrorCode::IncompatiblePack,
            "the pack targets a different desktop",
        )
        .with_context("desktop", &manifest.compatibility.desktop));
    }
    let constraint = PlasmaConstraint::parse(&manifest.compatibility.plasma).map_err(|_| {
        MorphixError::new(
            ErrorCode::IncompatiblePack,
            "the pack compatibility range is not usable",
        )
        .with_context("constraint", &manifest.compatibility.plasma)
    })?;
    let Some(version) = reads.plasma_version() else {
        return Ok(());
    };
    let parsed = crate::core::versions::PlasmaVersion::parse(version).map_err(|_| {
        MorphixError::new(
            ErrorCode::IncompatiblePack,
            "the detected Plasma version is not usable",
        )
        .with_context("version", version)
    })?;
    if constraint.matches(&parsed) {
        Ok(())
    } else {
        Err(MorphixError::new(
            ErrorCode::IncompatiblePack,
            "the pack is incompatible with this Plasma version",
        )
        .with_context("constraint", constraint.as_str())
        .with_context("version", version))
    }
}

/// Rewrites font-key operations to the owner the pack actually requested.
///
/// A font key may be claimed by either `appearance.fonts` or `settings.kde_global`
/// (PACK_SPEC.md 5.7); expansion emits the first owner, and this step moves the
/// operation to the requested one so the capability check sees the pack's intent.
fn normalize_font_capabilities(manifest: &Manifest, operations: &mut [ExpandedOp]) {
    let requested: BTreeSet<Capability> = manifest
        .capabilities
        .requires
        .iter()
        .filter_map(|id| Capability::from_id(id))
        .collect();
    for operation in operations.iter_mut() {
        let OperationAction::WriteConfigValue {
            file, group, key, ..
        } = &operation.action
        else {
            continue;
        };
        let Some(target) = crate::core::allowlist::ConfigTarget::parse(file) else {
            continue;
        };
        let owners = crate::core::allowlist::config_key_capabilities(target, group, key);
        if owners.len() < 2 {
            continue;
        }
        if !requested.contains(&operation.capability)
            && let Some(granted) = owners.iter().find(|owner| requested.contains(owner))
        {
            operation.capability = *granted;
        }
    }
}

/// Refuses undeclared capability use and unused requests.
fn check_capabilities(manifest: &Manifest, operations: &[ExpandedOp]) -> Result<()> {
    let requested: BTreeSet<Capability> = manifest
        .capabilities
        .requires
        .iter()
        .filter_map(|id| Capability::from_id(id))
        .collect();
    for operation in operations {
        if !requested.contains(&operation.capability) {
            return Err(MorphixError::new(
                ErrorCode::CapabilityDenied,
                "the plan uses a capability the pack did not declare",
            )
            .with_context("capability", operation.capability.as_str())
            .with_context("target", &operation.target.identity));
        }
    }
    let used: BTreeSet<Capability> = operations
        .iter()
        .map(|operation| operation.capability)
        .collect();
    for capability in &requested {
        if !used.contains(capability) {
            return Err(MorphixError::new(
                ErrorCode::CapabilityUnused,
                "the pack requests a capability nothing uses",
            )
            .with_context("capability", capability.as_str()));
        }
    }
    Ok(())
}

/// Refuses targets owned by another pack or present without ownership.
fn check_ownership(
    manifest: &Manifest,
    reads: &dyn PlannerReads,
    operations: &[ExpandedOp],
) -> Result<()> {
    let mut ownership = BTreeMap::new();
    for operation in operations {
        if let Some(relative) = operation.target.asset_path.as_deref() {
            let (kind, name) = asset_key_of(&operation.action);
            let state = reads.asset_ownership(kind, name, relative)?;
            ownership.insert(relative.to_owned(), state);
        }
    }
    check_asset_ownership(
        operations,
        &ownership,
        manifest.pack.id.as_str(),
        &manifest.pack.version.to_string(),
    )?;
    let installed: BTreeMap<String, (String, String, String)> = reads
        .installed_assets()
        .iter()
        .map(|asset| {
            (
                asset.target_path.clone(),
                (asset.pack_id.clone(), asset.kind.clone(), asset.key.clone()),
            )
        })
        .collect();
    check_installed_asset_collisions(operations, &installed, manifest.pack.id.as_str())?;
    check_dependencies(manifest, reads)?;
    Ok(())
}

/// The asset kind and installed name identifying one install operation.
fn asset_key_of(action: &OperationAction) -> (&str, &str) {
    match action {
        OperationAction::InstallAsset {
            kind,
            installed_name,
            ..
        } => (kind.as_str(), installed_name.as_str()),
        _ => ("", ""),
    }
}

/// Refuses unmet dependencies; records nothing when all are applied.
fn check_dependencies(manifest: &Manifest, reads: &dyn PlannerReads) -> Result<()> {
    let Some(dependencies) = &manifest.dependencies else {
        return Ok(());
    };
    for dependency in &dependencies.packs {
        let installed = reads.installed_pack_version(&dependency.id);
        let Some(version) = installed else {
            return Err(MorphixError::new(
                ErrorCode::DependencyMissing,
                "a declared dependency is not installed",
            )
            .with_context("dependency", &dependency.id));
        };
        let requirement = semver::VersionReq::parse(&dependency.version).map_err(|_| {
            MorphixError::new(
                ErrorCode::DependencyInvalid,
                "a declared dependency range is not usable",
            )
            .with_context("dependency", &dependency.id)
        })?;
        let installed_version =
            crate::core::versions::PackVersion::parse(&version).map_err(|_| {
                MorphixError::new(
                    ErrorCode::DependencyMissing,
                    "an installed dependency has an unusable version",
                )
                .with_context("dependency", &dependency.id)
            })?;
        if !requirement.matches(installed_version.as_semver()) {
            return Err(MorphixError::new(
                ErrorCode::DependencyMissing,
                "an installed dependency does not satisfy the required range",
            )
            .with_context("dependency", &dependency.id)
            .with_context("installed", version));
        }
        if !reads.pack_applied(&dependency.id) {
            return Err(MorphixError::new(
                ErrorCode::DependencyMissing,
                "a declared dependency is installed but not applied",
            )
            .with_context("dependency", &dependency.id));
        }
    }
    Ok(())
}

/// Drops `SetWallpaper` operations when the probe reports the channel unavailable.
///
/// The asset install is kept; the user finishes the step by hand (AC-24 branch).
fn apply_wallpaper_gate(reads: &dyn PlannerReads, operations: &mut Vec<ExpandedOp>) {
    if reads.wallpaper_supported() {
        return;
    }
    operations
        .retain(|operation| !matches!(operation.action, OperationAction::SetWallpaper { .. }));
}

/// Refuses operations whose capability the adapter reports as unavailable.
fn check_adapter_support(reads: &dyn PlannerReads, operations: &[ExpandedOp]) -> Result<()> {
    for operation in operations {
        if reads.capability_state(operation.capability) == "unavailable" {
            return Err(MorphixError::new(
                ErrorCode::CapabilityDenied,
                "the system does not support a capability this plan needs",
            )
            .with_context("capability", operation.capability.as_str())
            .with_context("target", &operation.target.identity));
        }
    }
    Ok(())
}

/// Captures pre-state, builds operations, preconditions, and warnings.
#[allow(clippy::too_many_lines)]
fn capture(
    reads: &dyn PlannerReads,
    manifest: &Manifest,
    operations: &[ExpandedOp],
) -> Result<(Vec<PlannedOperation>, Vec<Precondition>, Vec<PlanWarning>)> {
    let mut planned = Vec::with_capacity(operations.len());
    let mut preconditions = vec![
        Precondition::PackContent {
            pack_id: manifest.pack.id.as_str().to_owned(),
            content_hash: reads.content_hash().to_owned(),
        },
        Precondition::PlasmaCompatibility {
            version: reads.plasma_version().map(str::to_owned),
            constraint: manifest.compatibility.plasma.clone(),
            compatible: true,
        },
    ];
    let mut warnings = Vec::new();
    if reads.plasma_version().is_none() {
        warnings.push(PlanWarning {
            kind: WarningKind::ManualStep,
            message: "The Plasma version is unknown on this system; the plan was built \
                without a compatibility match. Apply remains available."
                .to_owned(),
            target: None,
        });
    }
    let wallpaper_dropped = manifest.requests(crate::core::capability::Capability::Wallpaper)
        && !reads.wallpaper_supported();
    if wallpaper_dropped {
        warnings.push(PlanWarning {
            kind: WarningKind::ManualStep,
            message: "Wallpaper cannot be set automatically on this system; the image is \
                installed and the desktop background is a manual step."
                .to_owned(),
            target: None,
        });
    }
    if manifest.requests(crate::core::capability::Capability::LookAndFeel) {
        warnings.push(PlanWarning {
            kind: WarningKind::ManualStep,
            message: "Global theme components resolve against the installed theme package \
                at apply time; per-component values are refined by the adapter."
                .to_owned(),
            target: None,
        });
    }

    for (index, operation) in operations.iter().enumerate() {
        let (previous, expected) = capture_op_state(reads, &operation.action)?;
        let overwrite = overwrites(&previous, &expected);
        preconditions.push(op_precondition(
            &operation.action,
            &operation.target.identity,
            &previous,
        ));
        if overwrite {
            warnings.push(PlanWarning {
                kind: WarningKind::OverwritesExistingValue,
                message: format!(
                    "{} already has a value; the plan shows the current and planned values.",
                    operation.target.display
                ),
                target: Some(operation.target.identity.clone()),
            });
        }
        if operation.verification == VerificationLevel::RestartVerified {
            warnings.push(PlanWarning {
                kind: WarningKind::RestartRequired,
                message: format!(
                    "{} takes effect after the shell restarts; Morphix never restarts it.",
                    operation.target.display
                ),
                target: Some(operation.target.identity.clone()),
            });
        }
        let description = describe(&operation.action, &previous, &expected);
        planned.push(PlannedOperation {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            capability: operation.capability,
            target: operation.target.clone(),
            action: operation.action.clone(),
            previous,
            expected,
            description,
            verification: operation.verification,
        });
    }
    if let Some(dependencies) = &manifest.dependencies {
        for dependency in &dependencies.packs {
            warnings.push(PlanWarning {
                kind: WarningKind::DependencyApplied,
                message: format!(
                    "Dependency {} is already applied; this plan builds on it.",
                    dependency.id
                ),
                target: None,
            });
        }
    }
    Ok((planned, preconditions, warnings))
}

/// Reads the previous state and derives the expected post-state for one operation.
fn capture_op_state(
    reads: &dyn PlannerReads,
    action: &OperationAction,
) -> Result<(Option<ResourceState>, ResourceState)> {
    match action {
        OperationAction::InstallAsset { tree_hash, .. } => Ok((
            Some(ResourceState::Absent),
            ResourceState::AssetPresent {
                hash: tree_hash.clone(),
            },
        )),
        OperationAction::WriteConfigValue {
            file,
            group,
            key,
            value,
        } => {
            let previous = reads
                .config_value(file, group, key)?
                .or(Some(ResourceState::Absent));
            Ok((previous, ResourceState::ConfigValue(value.clone())))
        }
        OperationAction::WriteShortcut {
            component,
            action: name,
            keys,
        } => {
            let previous = match reads.shortcut(component, name)? {
                Some(current) => Some(ResourceState::Shortcut { keys: current }),
                None => Some(ResourceState::Absent),
            };
            Ok((previous, ResourceState::Shortcut { keys: keys.clone() }))
        }
        OperationAction::SetKwinEffect { id, enabled } => {
            let previous = match reads.effect_enabled(id)? {
                Some(current) => Some(ResourceState::Effect { enabled: current }),
                None => Some(ResourceState::Absent),
            };
            Ok((previous, ResourceState::Effect { enabled: *enabled }))
        }
        OperationAction::SetWallpaper { image_path, .. } => {
            let previous = match reads.wallpaper()? {
                Some(image) => Some(ResourceState::Wallpaper { image }),
                None => Some(ResourceState::Absent),
            };
            Ok((
                previous,
                ResourceState::Wallpaper {
                    image: image_path.clone(),
                },
            ))
        }
        OperationAction::SetPanelProperty { value, .. } => Ok((
            None,
            ResourceState::ConfigValue(crate::desktop::kde::targets::ConfigValue::String(
                value.clone(),
            )),
        )),
        OperationAction::CreatePanel { panel_type } => Ok((
            Some(ResourceState::Absent),
            ResourceState::ConfigValue(crate::desktop::kde::targets::ConfigValue::String(format!(
                "created {panel_type}"
            ))),
        )),
        OperationAction::RemovePanel { .. } | OperationAction::RemoveWidget { .. } => {
            Ok((None, ResourceState::Absent))
        }
        OperationAction::AddWidget { widget_type, .. } => Ok((
            Some(ResourceState::Absent),
            ResourceState::ConfigValue(crate::desktop::kde::targets::ConfigValue::String(format!(
                "added {widget_type}"
            ))),
        )),
        OperationAction::ConfigureWidget { widget_type, .. } => Ok((
            None,
            ResourceState::ConfigValue(crate::desktop::kde::targets::ConfigValue::String(format!(
                "configured {widget_type}"
            ))),
        )),
        OperationAction::ApplyGlobalTheme { package_id, .. } => {
            let previous = reads
                .config_value("kdeglobals", "KDE", "LookAndFeelPackage")?
                .or(Some(ResourceState::Absent));
            Ok((
                previous,
                ResourceState::ConfigValue(crate::desktop::kde::targets::ConfigValue::String(
                    package_id.clone(),
                )),
            ))
        }
        OperationAction::ManageVirtualDesktop { name, .. } => Ok((
            Some(ResourceState::Absent),
            match name {
                Some(name) => ResourceState::ConfigValue(
                    crate::desktop::kde::targets::ConfigValue::String(name.clone()),
                ),
                None => ResourceState::Absent,
            },
        )),
    }
}

/// Whether the plan overwrites an existing value (drives the warning).
fn overwrites(previous: &Option<ResourceState>, expected: &ResourceState) -> bool {
    match previous {
        None | Some(ResourceState::Absent) => false,
        Some(before) => before != expected,
    }
}

/// Builds the drift-check precondition for one operation.
fn op_precondition(
    action: &OperationAction,
    identity: &str,
    previous: &Option<ResourceState>,
) -> Precondition {
    let state = previous
        .as_ref()
        .map(ResourceState::render)
        .unwrap_or_else(|| "(unknown)".to_owned());
    match action {
        OperationAction::InstallAsset { .. } => Precondition::AssetTarget {
            identity: identity.to_owned(),
            state: "absent".to_owned(),
        },
        OperationAction::WriteShortcut { .. } => Precondition::Shortcut {
            identity: identity.to_owned(),
            state,
        },
        OperationAction::SetKwinEffect { .. } => Precondition::Effect {
            identity: identity.to_owned(),
            state,
        },
        OperationAction::SetWallpaper { .. } => Precondition::Wallpaper {
            identity: identity.to_owned(),
            state,
        },
        _ => Precondition::ConfigValue {
            identity: identity.to_owned(),
            state,
        },
    }
}

/// Assembles the plan: capabilities, totals, expiry, and the content hash.
fn finish(
    manifest: &Manifest,
    reads: &dyn PlannerReads,
    operations: Vec<PlannedOperation>,
    preconditions: Vec<Precondition>,
    warnings: Vec<PlanWarning>,
    now_secs: i64,
) -> Result<ChangePlan> {
    let used: BTreeSet<Capability> = operations
        .iter()
        .map(|operation| operation.capability)
        .collect();
    let capabilities: Vec<Capability> = Capability::ALL
        .into_iter()
        .filter(|capability| used.contains(capability))
        .collect();
    let system = SystemFingerprint {
        plasma_version: reads.plasma_version().map(str::to_owned),
        session_type: reads.session_type().to_owned(),
        capability_states: Capability::ALL
            .into_iter()
            .map(|capability| {
                (
                    capability.as_str().to_owned(),
                    reads.capability_state(capability).to_owned(),
                )
            })
            .collect(),
    };
    let totals = PlanTotals {
        total_bytes: operations
            .iter()
            .map(|operation| match &operation.action {
                OperationAction::InstallAsset {
                    kind, asset_key, ..
                } => reads.asset_bytes(kind.as_str(), asset_key),
                _ => 0,
            })
            .sum(),
        asset_operations: operations
            .iter()
            .filter(|operation| matches!(operation.action, OperationAction::InstallAsset { .. }))
            .count(),
        config_operations: operations
            .iter()
            .filter(|operation| !matches!(operation.action, OperationAction::InstallAsset { .. }))
            .count(),
        operation_count: operations.len(),
    };
    let restart_required = operations
        .iter()
        .any(|operation| operation.verification == VerificationLevel::RestartVerified);
    let created_at = unix_to_rfc3339(now_secs);
    let expires_at = unix_to_rfc3339(now_secs.saturating_add(PLAN_TTL_SECONDS));
    let mut plan = ChangePlan {
        id: PlanId::generate(),
        hash: PlanHash::of("pending"),
        kind: PlanKind::ApplyPack,
        pack: Some(PackRef {
            id: manifest.pack.id.as_str().to_owned(),
            version: manifest.pack.version.to_string(),
            content_hash: reads.content_hash().to_owned(),
        }),
        snapshot_source: None,
        created_at,
        expires_at,
        system,
        capabilities,
        operations,
        preconditions,
        warnings,
        totals,
        restart_required,
    };
    plan.hash = PlanHash::of(&plan.canonical_json());
    Ok(plan)
}

/// Formats Unix seconds as RFC 3339 UTC without a date dependency.
///
/// `planning` may not depend on `storage`, so the small civil-date routine lives here
/// rather than reusing the database helper.
fn unix_to_rfc3339(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let remainder = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = remainder / 3_600;
    let minute = (remainder % 3_600) / 60;
    let second = remainder % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}
