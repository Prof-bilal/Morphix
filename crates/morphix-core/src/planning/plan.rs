//! Plan and operation models with canonical JSON and `PlanHash` (P3-01).
//!
//! **Ownership.** The persisted, immutable description of an intended change
//! (TRANSACTIONS.md §4). Models are pure data: no I/O, no clock reads, no adapter calls.
//! The planner builds them; the transaction engine (Phase 5) consumes them.
//!
//! **Allowed dependencies:** `errors`, `core`, `desktop` (for resolved targets).
//! **Must never depend on:** `transactions`, `storage`, Tauri.
//!
//! # Determinism
//!
//! [`ChangePlan::canonical_json`] serializes everything the plan *means* — kind, pack,
//! system fingerprint, capabilities, operations, preconditions, warnings, and totals —
//! with sorted keys and fixed field order. It excludes the plan ID and timestamps, so
//! planning twice on unchanged state yields identical content and an identical
//! [`PlanHash`]. [`PlanHash::of`] is SHA-256 over that canonical JSON, hex-encoded.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::capability::Capability;
use crate::core::resource::AssetKind;
use crate::desktop::kde::targets::{ConfigValue, ResolvedTarget, ResourceState};

/// Default plan time-to-live: ten minutes (TRANSACTIONS.md §4, ARCHITECTURE.md §7.2).
pub const PLAN_TTL_SECONDS: i64 = 600;

/// How long a generated ID's random suffix is, in hex characters.
const PLAN_ID_SUFFIX_HEX: usize = 16;

/// A persisted plan identifier, for example `plan_0193f29e_k3jqd8mz1p2a4b6c`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlanId(String);

impl PlanId {
    /// Generates a unique plan ID from the current time and a process-wide counter.
    ///
    /// Uniqueness never affects [`PlanHash`]: the ID is excluded from canonical JSON.
    #[must_use]
    pub fn generate() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let mut hasher = Sha256::new();
        hasher.update(nanos.to_be_bytes());
        hasher.update(counter.to_be_bytes());
        hasher.update(std::process::id().to_be_bytes());
        let digest = hasher.finalize();
        let suffix: String = digest[..PLAN_ID_SUFFIX_HEX / 2]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self(format!("plan_{suffix}"))
    }

    /// Wraps a validated plan ID string.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// The identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PlanId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// SHA-256 over the canonical plan JSON, hex-encoded (TRANSACTIONS.md §4).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlanHash(String);

impl PlanHash {
    /// Hashes canonical plan JSON.
    #[must_use]
    pub fn of(canonical_json: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(canonical_json.as_bytes());
        let digest = hasher.finalize();
        let mut output = String::with_capacity(digest.len() * 2);
        for byte in digest {
            output.push_str(&format!("{byte:02x}"));
        }
        Self(output)
    }

    /// The hex digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PlanHash {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// What a plan intends to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    /// Apply a validated pack.
    ApplyPack,
    /// Return snapshot resources to their recorded state (Phase 4 wires the command).
    RestoreSnapshot,
}

impl PlanKind {
    /// The database spelling (`DATABASE.md` plans.kind).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApplyPack => "apply_pack",
            Self::RestoreSnapshot => "restore_snapshot",
        }
    }
}

/// Which pack a plan applies, pinned to the content that was planned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRef {
    /// Manifest pack ID.
    pub id: String,
    /// Strict SemVer version string.
    pub version: String,
    /// Deterministic tree hash pinned at plan time.
    pub content_hash: String,
}

/// The detected system a plan was built against.
///
/// `capability_states` records the planner-visible adapter support per capability ID
/// (`available`, `degraded`, `unavailable`); an unreported capability is treated as
/// unavailable, matching [`crate::desktop::probe::CapabilityMatrix`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemFingerprint {
    /// Detected Plasma version, if any probe path reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plasma_version: Option<String>,
    /// Display session (`wayland`, `x11`, `tty`, `unknown`).
    pub session_type: String,
    /// Capability ID to support state, in documentation order.
    pub capability_states: BTreeMap<String, String>,
}

/// One closed operation the engine can apply, verify, and reverse.
///
/// Variants follow TRANSACTIONS.md §5. `ApplyGlobalTheme` always carries its decomposed
/// primitive operations so every component stays individually reversible; the marker is
/// part of `expanded`, never a bare side effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum OperationAction {
    /// Stage, hash, rename, and verify one asset; remove on reverse.
    InstallAsset {
        /// Which asset kind this installs.
        kind: AssetKind,
        /// Manifest asset key.
        asset_key: String,
        /// Engine-derived on-disk name.
        installed_name: String,
        /// Engine-relative target path (resolved through the target map).
        target: String,
        /// Expected content hash after install.
        tree_hash: String,
        /// Whether the target is a directory tree.
        is_directory: bool,
    },
    /// Read-modify-write one allowlisted config key.
    WriteConfigValue {
        /// KDE config file (`kdeglobals`, `kwinrc`, `plasmarc`, `kcminputrc`).
        file: String,
        /// KConfig group.
        group: String,
        /// KConfig key.
        key: String,
        /// Value to write.
        value: ConfigValue,
    },
    /// Write one shortcut binding and sync it through `kglobalaccel`.
    WriteShortcut {
        /// Allowlisted component (`kwin`, `plasmashell`).
        component: String,
        /// Allowlisted action name.
        action: String,
        /// One to four chords.
        keys: Vec<String>,
    },
    /// Toggle one allowlisted KWin effect (config plus load/unload pairing).
    SetKwinEffect {
        /// Allowlisted effect ID.
        id: String,
        /// Desired state.
        enabled: bool,
    },
    /// Install a wallpaper image and set it through the constrained script.
    SetWallpaper {
        /// Owning pack ID.
        pack_id: String,
        /// Engine-relative installed image path.
        image_path: String,
        /// Desktop scope; V0.1 always plans `all`.
        desktop: String,
    },
    /// Set one panel property through Plasma scripting with read-back.
    SetPanelProperty {
        /// Target panel ID.
        panel_id: u32,
        /// Closed property name (`location`, `height`, `alignment`, `floating`, `hiding`).
        property: String,
        /// New value, rendered as Plasma scripting expects it.
        value: String,
    },
    /// Create a panel and record its assigned ID at apply time.
    CreatePanel {
        /// Panel type; V0.1 only plans `org.kde.panel`.
        panel_type: String,
    },
    /// Remove a panel after capturing its properties and widget list.
    RemovePanel {
        /// Target panel ID.
        panel_id: u32,
    },
    /// Add an allowlisted widget to a panel.
    AddWidget {
        /// Target panel ID.
        panel_id: u32,
        /// Curated widget type.
        widget_type: String,
        /// Append position; Plasma assigns the final index.
        index: i32,
    },
    /// Configure one widget key (only planned when the manifest names a value).
    ConfigureWidget {
        /// Target panel ID.
        panel_id: u32,
        /// Target widget ID from the live inventory.
        widget_id: u32,
        /// Widget type, for the human-readable description.
        widget_type: String,
    },
    /// Remove a widget after capturing its type, index, and config.
    RemoveWidget {
        /// Target panel ID.
        panel_id: u32,
        /// Target widget ID from the live inventory.
        widget_id: u32,
        /// Widget type, for the human-readable description.
        widget_type: String,
    },
    /// Apply a global theme as decomposed primitives (TRANSACTIONS.md §5).
    ApplyGlobalTheme {
        /// Installed look-and-feel package ID.
        package_id: String,
        /// Requested components, in manifest order.
        components: Vec<String>,
        /// The primitive operations, including the marker write.
        expanded: Vec<OperationAction>,
    },
    /// Create, rename, or remove a virtual desktop.
    ManageVirtualDesktop {
        /// Closed action (`add`, `rename`, `remove`).
        action: String,
        /// Desktop name for `add`/`rename`, or the removal target.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

impl OperationAction {
    /// Stable snake_case name used in diffs, logs, and the declared-change audit.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::InstallAsset { .. } => "install_asset",
            Self::WriteConfigValue { .. } => "write_config_value",
            Self::WriteShortcut { .. } => "write_shortcut",
            Self::SetKwinEffect { .. } => "set_kwin_effect",
            Self::SetWallpaper { .. } => "set_wallpaper",
            Self::SetPanelProperty { .. } => "set_panel_property",
            Self::CreatePanel { .. } => "create_panel",
            Self::RemovePanel { .. } => "remove_panel",
            Self::AddWidget { .. } => "add_widget",
            Self::ConfigureWidget { .. } => "configure_widget",
            Self::RemoveWidget { .. } => "remove_widget",
            Self::ApplyGlobalTheme { .. } => "apply_global_theme",
            Self::ManageVirtualDesktop { .. } => "manage_virtual_desktop",
        }
    }

    /// The capability that must grant this operation.
    #[must_use]
    pub fn capability(&self) -> Capability {
        match self {
            Self::InstallAsset { kind, .. } => match kind {
                AssetKind::ColorScheme => Capability::ColorScheme,
                AssetKind::IconTheme => Capability::Icons,
                AssetKind::CursorTheme => Capability::Cursors,
                AssetKind::PlasmaStyle => Capability::PlasmaStyle,
                AssetKind::WindowDecoration => Capability::WindowDecorations,
                AssetKind::Wallpaper => Capability::Wallpaper,
            },
            Self::WriteConfigValue { .. } => Capability::KdeGlobal,
            Self::WriteShortcut { .. } => Capability::Shortcuts,
            Self::SetKwinEffect { .. } => Capability::KwinEffects,
            Self::SetWallpaper { .. } => Capability::Wallpaper,
            Self::SetPanelProperty { .. } | Self::RemovePanel { .. } => Capability::PanelProperties,
            Self::CreatePanel { .. } => Capability::PanelCreate,
            Self::AddWidget { .. } | Self::ConfigureWidget { .. } | Self::RemoveWidget { .. } => {
                Capability::PanelWidgets
            }
            Self::ApplyGlobalTheme { .. } => Capability::LookAndFeel,
            Self::ManageVirtualDesktop { .. } => Capability::DesktopBehavior,
        }
    }
}

/// How the engine will verify an operation after apply (ARCHITECTURE.md §9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationLevel {
    /// Re-read persisted config/asset state and compare with `expected`.
    StateVerified,
    /// Confirm through the owning daemon's live channel as well.
    LiveVerified,
    /// Correct only after a plasmashell restart; applied to config, never live-verified.
    RestartVerified,
}

/// One planned mutation with its reverse data captured up front.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedOperation {
    /// Position in the deterministic apply order.
    pub index: u32,
    /// The capability that grants this operation.
    pub capability: Capability,
    /// Engine-resolved target; never pack-supplied.
    pub target: ResolvedTarget,
    /// What the engine will do.
    pub action: OperationAction,
    /// Observed pre-state; `None` only when the read found nothing to record.
    pub previous: Option<ResourceState>,
    /// Post-state the engine will verify.
    pub expected: ResourceState,
    /// User-facing description generated by the engine.
    pub description: String,
    /// How apply will verify this operation.
    pub verification: VerificationLevel,
}

/// One precondition binding a plan to the state it was built against.
///
/// At apply time every precondition is re-evaluated inside the lock; any mismatch
/// returns `E_PLAN_STALE` and writes nothing (ARCHITECTURE.md §7.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "precondition")]
pub enum Precondition {
    /// The pack content that was planned.
    PackContent {
        /// Manifest pack ID.
        pack_id: String,
        /// Tree hash pinned at plan time.
        content_hash: String,
    },
    /// The compatibility verdict that was planned.
    PlasmaCompatibility {
        /// Detected version, if any.
        version: Option<String>,
        /// Manifest range expression.
        constraint: String,
        /// Whether the range matched.
        compatible: bool,
    },
    /// A config key's observed value.
    ConfigValue {
        /// Conflict-scan identity.
        identity: String,
        /// Rendered pre-state (`(absent)` when the key did not exist).
        state: String,
    },
    /// An asset target's observed presence.
    AssetTarget {
        /// Conflict-scan identity.
        identity: String,
        /// `absent`, `owned:<pack-id>`, or `unowned`.
        state: String,
    },
    /// A shortcut's observed binding.
    Shortcut {
        /// Conflict-scan identity.
        identity: String,
        /// Rendered pre-state.
        state: String,
    },
    /// A KWin effect's observed state.
    Effect {
        /// Conflict-scan identity.
        identity: String,
        /// `enabled` or `disabled`.
        state: String,
    },
    /// A wallpaper descriptor's observed value.
    Wallpaper {
        /// Conflict-scan identity.
        identity: String,
        /// Rendered pre-state.
        state: String,
    },
}

/// The closed set of plan warnings (TRANSACTIONS.md §4.1).
///
/// Warnings never block apply; they are displayed and acknowledged through the same
/// consent. Anything that could make the result unsafe is an error, not a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    /// The plan overwrites a value the user already set; the old value is shown.
    OverwritesExistingValue,
    /// The change takes effect after a restart or next login.
    RestartRequired,
    /// The engine cannot complete this step; the user finishes it by hand.
    ManualStep,
    /// A declared dependency was already applied.
    DependencyApplied,
}

impl WarningKind {
    /// Stable identifier for logs and the UI contract.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OverwritesExistingValue => "overwrites_existing_value",
            Self::RestartRequired => "restart_required",
            Self::ManualStep => "manual_step",
            Self::DependencyApplied => "dependency_applied",
        }
    }
}

/// One displayed warning attached to a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanWarning {
    /// Which warning this is.
    pub kind: WarningKind,
    /// User-facing explanation naming the affected resource.
    pub message: String,
    /// Conflict-scan identity of the affected resource, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Install footprint and operation counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanTotals {
    /// Bytes of asset content the plan installs.
    pub total_bytes: u64,
    /// Total planned operations.
    pub operation_count: usize,
    /// Operations that install assets.
    pub asset_operations: usize,
    /// Operations that write config, shortcuts, or effects.
    pub config_operations: usize,
}

/// A persisted, immutable description of an intended change (TRANSACTIONS.md §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePlan {
    /// Surrogate ID; excluded from the content hash.
    pub id: PlanId,
    /// SHA-256 over the canonical JSON; consent binds to this value.
    pub hash: PlanHash,
    /// What the plan does.
    pub kind: PlanKind,
    /// The pack being applied, if this is a pack plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<PackRef>,
    /// Snapshot being restored, if this is a restore plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_source: Option<String>,
    /// RFC 3339 creation time; excluded from the content hash.
    pub created_at: String,
    /// RFC 3339 expiry; excluded from the content hash.
    pub expires_at: String,
    /// The system the plan was built against.
    pub system: SystemFingerprint,
    /// Granted capabilities, in documentation order.
    pub capabilities: Vec<Capability>,
    /// Deterministic, ordered operations.
    pub operations: Vec<PlannedOperation>,
    /// State the plan is bound to.
    pub preconditions: Vec<Precondition>,
    /// Displayed warnings.
    pub warnings: Vec<PlanWarning>,
    /// Footprint summary.
    pub totals: PlanTotals,
    /// Whether any operation needs a restart or next login to take effect.
    pub restart_required: bool,
}

impl ChangePlan {
    /// Canonical JSON: everything the plan means, with sorted keys and fixed order.
    ///
    /// Excludes the surrogate ID and timestamps so unchanged state plans identically.
    #[must_use]
    pub fn canonical_json(&self) -> String {
        let canonical = CanonicalPlan::from(self);
        serde_json::to_string(&canonical)
            .unwrap_or_else(|_| "{\"error\":\"canonical plan is not serializable\"}".to_owned())
    }

    /// Recomputes the content hash and reports whether it matches the stored one.
    #[must_use]
    pub fn hash_is_current(&self) -> bool {
        PlanHash::of(&self.canonical_json()) == self.hash
    }

    /// Whether the plan expired relative to `now_rfc3339`.
    #[must_use]
    pub fn is_expired(&self, now_rfc3339: &str) -> bool {
        self.expires_at.as_str() < now_rfc3339
    }
}

/// The hash-relevant projection of a plan, in fixed field order with sorted maps.
///
/// `SystemFingerprint.capability_states` is already a `BTreeMap`; every other field is
/// a struct or a deterministically ordered `Vec`, so `serde_json` output is stable.
#[derive(Debug, Clone, Serialize)]
struct CanonicalPlan<'a> {
    kind: PlanKind,
    pack: Option<&'a PackRef>,
    snapshot_source: Option<&'a String>,
    system: &'a SystemFingerprint,
    capabilities: &'a [Capability],
    operations: &'a [PlannedOperation],
    preconditions: &'a [Precondition],
    warnings: &'a [PlanWarning],
    totals: &'a PlanTotals,
    restart_required: bool,
}

impl<'a> From<&'a ChangePlan> for CanonicalPlan<'a> {
    fn from(plan: &'a ChangePlan) -> Self {
        Self {
            kind: plan.kind,
            pack: plan.pack.as_ref(),
            snapshot_source: plan.snapshot_source.as_ref(),
            system: &plan.system,
            capabilities: &plan.capabilities,
            operations: &plan.operations,
            preconditions: &plan.preconditions,
            warnings: &plan.warnings,
            totals: &plan.totals,
            restart_required: plan.restart_required,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::collections::BTreeMap;

    use super::{
        ChangePlan, OperationAction, PLAN_TTL_SECONDS, PlanHash, PlanId, PlanKind, PlanTotals,
        PlannedOperation, Precondition, SystemFingerprint, VerificationLevel, WarningKind,
    };
    use crate::core::capability::Capability;
    use crate::core::resource::AssetKind;
    use crate::desktop::kde::targets::{ConfigValue, ResolvedTarget, ResourceState};

    pub fn sample_plan() -> ChangePlan {
        let operation = PlannedOperation {
            index: 0,
            capability: Capability::ColorScheme,
            target: ResolvedTarget::config_key(
                crate::core::allowlist::ConfigTarget::Kdeglobals,
                "General",
                "ColorScheme",
            ),
            action: OperationAction::InstallAsset {
                kind: AssetKind::ColorScheme,
                asset_key: "dark".to_owned(),
                installed_name: "cupertino-dark".to_owned(),
                target: "color-schemes/cupertino-dark.colors".to_owned(),
                tree_hash: "abc123".to_owned(),
                is_directory: false,
            },
            previous: Some(ResourceState::ConfigValue(ConfigValue::String(
                "BreezeDark".to_owned(),
            ))),
            expected: ResourceState::ConfigValue(ConfigValue::String("cupertino-dark".to_owned())),
            description: "Install color scheme dark".to_owned(),
            verification: VerificationLevel::StateVerified,
        };
        let system = SystemFingerprint {
            plasma_version: Some("6.8.0".to_owned()),
            session_type: "wayland".to_owned(),
            capability_states: BTreeMap::from([(
                "appearance.color_scheme".to_owned(),
                "available".to_owned(),
            )]),
        };
        let mut plan = ChangePlan {
            id: PlanId::new("plan_test".to_owned()),
            hash: PlanHash::of("placeholder"),
            kind: PlanKind::ApplyPack,
            pack: None,
            snapshot_source: None,
            created_at: "2026-10-21T00:00:00Z".to_owned(),
            expires_at: "2026-10-21T00:10:00Z".to_owned(),
            system,
            capabilities: vec![Capability::ColorScheme],
            operations: vec![operation],
            preconditions: vec![Precondition::PackContent {
                pack_id: "dev.morphix.cupertino".to_owned(),
                content_hash: "abc".to_owned(),
            }],
            warnings: Vec::new(),
            totals: PlanTotals {
                total_bytes: 10,
                operation_count: 1,
                asset_operations: 1,
                config_operations: 0,
            },
            restart_required: false,
        };
        plan.hash = PlanHash::of(&plan.canonical_json());
        plan
    }

    #[test]
    fn canonical_json_is_stable_and_hash_verified() {
        let first = sample_plan();
        let mut second = first.clone();
        second.id = PlanId::new("plan_different_id".to_owned());
        second.created_at = "2027-01-01T00:00:00Z".to_owned();
        second.expires_at = "2027-01-01T00:10:00Z".to_owned();

        assert_eq!(first.canonical_json(), second.canonical_json());
        assert!(first.hash_is_current());
        assert_eq!(first.hash, PlanHash::of(&second.canonical_json()));
    }

    #[test]
    fn any_meaningful_change_alters_the_hash() {
        let first = sample_plan();
        let mut second = first.clone();
        second.restart_required = true;
        assert_ne!(first.canonical_json(), second.canonical_json());
        assert!(!second.hash_is_current());
    }

    #[test]
    fn generated_ids_are_unique_and_expiry_spans_ten_minutes() {
        assert_eq!(PLAN_TTL_SECONDS, 600);
        let first = PlanId::generate();
        let second = PlanId::generate();
        assert_ne!(first, second);
        assert!(first.as_str().starts_with("plan_"));
    }

    #[test]
    fn warning_kinds_use_the_documented_ids() {
        assert_eq!(
            WarningKind::OverwritesExistingValue.as_str(),
            "overwrites_existing_value"
        );
        assert_eq!(WarningKind::RestartRequired.as_str(), "restart_required");
        assert_eq!(WarningKind::ManualStep.as_str(), "manual_step");
        assert_eq!(
            WarningKind::DependencyApplied.as_str(),
            "dependency_applied"
        );
    }

    #[test]
    fn operations_report_their_owning_capability() {
        let plan = sample_plan();
        assert_eq!(
            plan.operations[0].action.capability(),
            Capability::ColorScheme
        );
        assert_eq!(plan.operations[0].action.name(), "install_asset");
        assert_eq!(PlanKind::ApplyPack.as_str(), "apply_pack");
    }
}
