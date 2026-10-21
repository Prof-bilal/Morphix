//! Read-only pre-state seam for the planner (P3-02).
//!
//! **Ownership.** The planner reads live system state only through [`PlannerReads`], a
//! trait this module owns. The trait has no write, delete, rename, or DBus-mutating
//! method, so planning purity is enforced by types and asserted by the recording test in
//! P3-12: any future mutating method would be a compile-visible addition here.
//!
//! The production implementation that talks to the real KDE adapter arrives with the
//! Phase 6 adapter reads; until then the `create_pack_plan` command answers
//! `E_ADAPTER_UNAVAILABLE` rather than planning against guessed state
//! ([ADR-024](DECISIONS.md#adr-024--acceptance-criteria-follow-the-phase-that-supplies-their-dependency)).
//!
//! **Allowed dependencies:** `errors`, `core`, `desktop`, `packs`.
//! **Must never depend on:** `transactions`, `storage`.

use std::collections::BTreeMap;

use crate::core::capability::Capability;
use crate::desktop::kde::targets::ResourceState;
use crate::errors::Result;
use crate::packs::manifest::Manifest;

/// Whether an asset's install target already exists and who owns it.
///
/// The ownership snapshot is passed in by the service facade, so `planning` keeps its
/// ban on depending on `storage` (ARCHITECTURE.md §5, TASKS.md P3-06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetOwnership {
    /// Nothing exists at the target path.
    Absent,
    /// Owned by the named pack at the named version; replacing it is planned.
    OwnedBy {
        /// Owning pack ID.
        pack_id: String,
        /// Owning pack version.
        version: String,
        /// Existing tree or file hash.
        hash: String,
    },
    /// Something exists that no pack owns; planning refuses to overwrite it.
    UnownedPresent {
        /// Existing tree or file hash, when known.
        hash: Option<String>,
    },
}

impl TargetOwnership {
    /// Renders the precondition state token (`absent`, `owned:<pack>`, `unowned`).
    #[must_use]
    pub fn state_token(&self) -> String {
        match self {
            Self::Absent => "absent".to_owned(),
            Self::OwnedBy { pack_id, .. } => format!("owned:{pack_id}"),
            Self::UnownedPresent { .. } => "unowned".to_owned(),
        }
    }
}

/// One installed asset record the facade passes in for ownership checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledAssetView {
    /// Owning pack ID.
    pub pack_id: String,
    /// Owning pack version.
    pub version: String,
    /// Asset kind ID (`color_scheme`, …).
    pub kind: String,
    /// Manifest asset key.
    pub key: String,
    /// Engine-relative target path.
    pub target_path: String,
    /// Pinned tree hash.
    pub tree_hash: String,
}

/// One live panel for panel-operation planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    /// Live panel ID.
    pub id: u32,
    /// Widget types in order.
    pub widgets: Vec<WidgetView>,
}

/// One live widget for widget-operation planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetView {
    /// Live widget ID (Plasma reassigns these; restore is by type and order).
    pub id: u32,
    /// Curated widget type.
    pub widget_type: String,
}

/// One virtual desktop for desktop-behaviour planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualDesktopView {
    /// Live desktop ID, when known.
    pub id: Option<String>,
    /// Desktop name.
    pub name: String,
    /// Position in the desktop list.
    pub position: u32,
}

/// Read-only system and target pre-state for one planning run.
///
/// Every method is a read. There is deliberately no method that writes, deletes,
/// renames, notifies, or reconfigures anything; the P3-12 purity test fails if a
/// mutating method is added without updating the recording assertion.
pub trait PlannerReads {
    /// The validated manifest being planned.
    ///
    /// # Errors
    ///
    /// Returns `E_INTERNAL` when the seam was built without a manifest; a planner run
    /// without one is a programming error, never a user-facing refusal.
    fn manifest(&self) -> Result<&Manifest>;

    /// The pack's pinned content hash (manifest plus checksums).
    fn content_hash(&self) -> &str;

    /// Detected Plasma version, if any probe path reported one.
    fn plasma_version(&self) -> Option<&str>;

    /// Display session (`wayland`, `x11`, `tty`, `unknown`).
    fn session_type(&self) -> &str;

    /// Planner-visible adapter support per capability.
    ///
    /// Missing entries read as `unavailable`; the planner never treats silence as
    /// support.
    fn capability_state(&self, capability: Capability) -> &'static str;

    /// Whether the wallpaper write path and its independent read-back channel work.
    fn wallpaper_supported(&self) -> bool;

    /// Current value of a config key, or `None` when the key is absent.
    fn config_value(&self, file: &str, group: &str, key: &str) -> Result<Option<ResourceState>>;

    /// Whole-file hash of a config file, or `None` when the file is absent.
    fn config_file_hash(&self, file: &str) -> Result<Option<String>>;

    /// Ownership of an asset install target.
    fn asset_ownership(
        &self,
        kind: &str,
        installed_name: &str,
        target: &str,
    ) -> Result<TargetOwnership>;

    /// Installed-asset index for collision checks against other packs.
    fn installed_assets(&self) -> &[InstalledAssetView];

    /// Installed size of one declared asset, in bytes.
    fn asset_bytes(&self, kind: &str, key: &str) -> u64;

    /// Current binding of a shortcut action, or `None` when unbound.
    fn shortcut(&self, component: &str, action: &str) -> Result<Option<Vec<String>>>;

    /// Current enabled state of a KWin effect.
    fn effect_enabled(&self, id: &str) -> Result<Option<bool>>;

    /// Current wallpaper descriptor for the planned desktop scope.
    fn wallpaper(&self) -> Result<Option<String>>;

    /// Whether a look-and-feel package is installed.
    fn look_and_feel_installed(&self, package_id: &str) -> bool;

    /// Live panels in order, for panel-operation planning.
    fn panels(&self) -> &[PanelView];

    /// Live virtual desktops in order.
    fn virtual_desktops(&self) -> &[VirtualDesktopView];

    /// Installed version of a dependency pack, if present.
    fn installed_pack_version(&self, pack_id: &str) -> Option<String>;

    /// Whether a dependency pack is currently applied.
    fn pack_applied(&self, pack_id: &str) -> bool;
}

/// In-memory [`PlannerReads`] for planner tests and fixtures.
///
/// Backed by plain maps; every accessor records its name in [`MemReads::calls`] so the
/// purity test can assert that planning only performed reads.
#[derive(Debug, Default)]
pub struct MemReads {
    /// Manifest being planned.
    pub manifest: Option<Manifest>,
    /// Pinned content hash.
    pub content_hash: String,
    /// Detected Plasma version.
    pub plasma_version: Option<String>,
    /// Display session.
    pub session_type: String,
    /// Capability ID to support state.
    pub capability_states: BTreeMap<String, String>,
    /// Wallpaper channel availability.
    pub wallpaper_supported: bool,
    /// Config values by `(file, group, key)`.
    pub config_values: BTreeMap<(String, String, String), ResourceState>,
    /// Config file hashes by file name.
    pub config_hashes: BTreeMap<String, String>,
    /// Asset ownership by target path.
    pub ownership: BTreeMap<String, TargetOwnership>,
    /// Installed-asset index.
    pub assets: Vec<InstalledAssetView>,
    /// Asset sizes by `(kind, key)`.
    pub sizes: BTreeMap<(String, String), u64>,
    /// Shortcut bindings by `(component, action)`.
    pub shortcuts: BTreeMap<(String, String), Vec<String>>,
    /// Effect states by ID.
    pub effects: BTreeMap<String, bool>,
    /// Current wallpaper descriptor.
    pub wallpaper_descriptor: Option<String>,
    /// Installed look-and-feel packages.
    pub look_and_feel_packages: Vec<String>,
    /// Live panels.
    pub panel_list: Vec<PanelView>,
    /// Live virtual desktops.
    pub desktops: Vec<VirtualDesktopView>,
    /// Installed pack versions by ID.
    pub installed_packs: BTreeMap<String, String>,
    /// Applied pack IDs.
    pub applied_packs: Vec<String>,
}

impl MemReads {
    /// Marks every capability available (tests narrow this per case).
    pub fn all_capabilities_available(&mut self) {
        for capability in Capability::ALL {
            self.capability_states
                .insert(capability.as_str().to_owned(), "available".to_owned());
        }
    }
}

impl PlannerReads for MemReads {
    fn manifest(&self) -> Result<&Manifest> {
        self.manifest.as_ref().ok_or_else(|| {
            crate::errors::MorphixError::new(
                crate::errors::ErrorCode::Internal,
                "the planner reads were built without a manifest",
            )
        })
    }

    fn content_hash(&self) -> &str {
        &self.content_hash
    }

    fn plasma_version(&self) -> Option<&str> {
        self.plasma_version.as_deref()
    }

    fn session_type(&self) -> &str {
        &self.session_type
    }

    fn capability_state(&self, capability: Capability) -> &'static str {
        match self
            .capability_states
            .get(capability.as_str())
            .map(String::as_str)
        {
            Some("available") => "available",
            Some("degraded") => "degraded",
            _ => "unavailable",
        }
    }

    fn wallpaper_supported(&self) -> bool {
        self.wallpaper_supported
    }

    fn config_value(&self, file: &str, group: &str, key: &str) -> Result<Option<ResourceState>> {
        Ok(self
            .config_values
            .get(&(file.to_owned(), group.to_owned(), key.to_owned()))
            .cloned())
    }

    fn config_file_hash(&self, file: &str) -> Result<Option<String>> {
        Ok(self.config_hashes.get(file).cloned())
    }

    fn asset_ownership(
        &self,
        _kind: &str,
        _installed_name: &str,
        target: &str,
    ) -> Result<TargetOwnership> {
        Ok(self
            .ownership
            .get(target)
            .cloned()
            .unwrap_or(TargetOwnership::Absent))
    }

    fn installed_assets(&self) -> &[InstalledAssetView] {
        &self.assets
    }

    fn asset_bytes(&self, kind: &str, key: &str) -> u64 {
        self.sizes
            .get(&(kind.to_owned(), key.to_owned()))
            .copied()
            .unwrap_or(0)
    }

    fn shortcut(&self, component: &str, action: &str) -> Result<Option<Vec<String>>> {
        Ok(self
            .shortcuts
            .get(&(component.to_owned(), action.to_owned()))
            .cloned())
    }

    fn effect_enabled(&self, id: &str) -> Result<Option<bool>> {
        Ok(self.effects.get(id).copied())
    }

    fn wallpaper(&self) -> Result<Option<String>> {
        Ok(self.wallpaper_descriptor.clone())
    }

    fn look_and_feel_installed(&self, package_id: &str) -> bool {
        self.look_and_feel_packages
            .iter()
            .any(|package| package == package_id)
    }

    fn panels(&self) -> &[PanelView] {
        &self.panel_list
    }

    fn virtual_desktops(&self) -> &[VirtualDesktopView] {
        &self.desktops
    }

    fn installed_pack_version(&self, pack_id: &str) -> Option<String> {
        self.installed_packs.get(pack_id).cloned()
    }

    fn pack_applied(&self, pack_id: &str) -> bool {
        self.applied_packs.iter().any(|id| id == pack_id)
    }
}

/// A recording wrapper that proves planning performs only reads (P3-12).
///
/// It delegates every call to an inner [`MemReads`] and records the call name.
/// [`PlannerReads`] exposes no write, delete, rename, or notify method, so a plan built
/// through this wrapper can only have observed state; the purity test asserts the log
/// contains read names alone and that a sentinel tree the planner could see is
/// byte-identical afterwards.
#[derive(Debug)]
pub struct RecordingReads<'a> {
    inner: &'a MemReads,
    log: std::cell::RefCell<Vec<String>>,
}

impl<'a> RecordingReads<'a> {
    /// Wraps inner state for one planning run.
    #[must_use]
    pub fn new(inner: &'a MemReads) -> Self {
        Self {
            inner,
            log: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Recorded read calls, oldest first. No write entry can exist: the trait offers no
    /// mutating method to record.
    #[must_use]
    pub fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn note(&self, name: impl Into<String>) {
        self.log.borrow_mut().push(name.into());
    }
}

impl PlannerReads for RecordingReads<'_> {
    fn manifest(&self) -> Result<&Manifest> {
        self.note("manifest");
        self.inner.manifest()
    }

    fn content_hash(&self) -> &str {
        self.note("content_hash");
        self.inner.content_hash()
    }

    fn plasma_version(&self) -> Option<&str> {
        self.note("plasma_version");
        self.inner.plasma_version()
    }

    fn session_type(&self) -> &str {
        self.note("session_type");
        self.inner.session_type()
    }

    fn capability_state(&self, capability: Capability) -> &'static str {
        self.note(format!("capability_state:{}", capability.as_str()));
        self.inner.capability_state(capability)
    }

    fn wallpaper_supported(&self) -> bool {
        self.note("wallpaper_supported");
        self.inner.wallpaper_supported()
    }

    fn config_value(&self, file: &str, group: &str, key: &str) -> Result<Option<ResourceState>> {
        self.note(format!("config_value:{file}:{group}:{key}"));
        self.inner.config_value(file, group, key)
    }

    fn config_file_hash(&self, file: &str) -> Result<Option<String>> {
        self.note(format!("config_file_hash:{file}"));
        self.inner.config_file_hash(file)
    }

    fn asset_ownership(
        &self,
        kind: &str,
        installed_name: &str,
        target: &str,
    ) -> Result<TargetOwnership> {
        self.note(format!("asset_ownership:{kind}:{installed_name}"));
        self.inner.asset_ownership(kind, installed_name, target)
    }

    fn installed_assets(&self) -> &[InstalledAssetView] {
        self.note("installed_assets");
        self.inner.installed_assets()
    }

    fn asset_bytes(&self, kind: &str, key: &str) -> u64 {
        self.note(format!("asset_bytes:{kind}:{key}"));
        self.inner.asset_bytes(kind, key)
    }

    fn shortcut(&self, component: &str, action: &str) -> Result<Option<Vec<String>>> {
        self.note(format!("shortcut:{component}:{action}"));
        self.inner.shortcut(component, action)
    }

    fn effect_enabled(&self, id: &str) -> Result<Option<bool>> {
        self.note(format!("effect_enabled:{id}"));
        self.inner.effect_enabled(id)
    }

    fn wallpaper(&self) -> Result<Option<String>> {
        self.note("wallpaper");
        self.inner.wallpaper()
    }

    fn look_and_feel_installed(&self, package_id: &str) -> bool {
        self.note(format!("look_and_feel_installed:{package_id}"));
        self.inner.look_and_feel_installed(package_id)
    }

    fn panels(&self) -> &[PanelView] {
        self.note("panels");
        self.inner.panels()
    }

    fn virtual_desktops(&self) -> &[VirtualDesktopView] {
        self.note("virtual_desktops");
        self.inner.virtual_desktops()
    }

    fn installed_pack_version(&self, pack_id: &str) -> Option<String> {
        self.note(format!("installed_pack_version:{pack_id}"));
        self.inner.installed_pack_version(pack_id)
    }

    fn pack_applied(&self, pack_id: &str) -> bool {
        self.note(format!("pack_applied:{pack_id}"));
        self.inner.pack_applied(pack_id)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{MemReads, PlannerReads, TargetOwnership};
    use crate::core::capability::Capability;

    #[test]
    fn unreported_capabilities_read_as_unavailable() {
        let reads = MemReads::default();
        assert_eq!(reads.capability_state(Capability::Wallpaper), "unavailable");
    }

    #[test]
    fn ownership_tokens_are_stable() {
        assert_eq!(TargetOwnership::Absent.state_token(), "absent");
        assert_eq!(
            TargetOwnership::OwnedBy {
                pack_id: "dev.morphix.a".to_owned(),
                version: "0.1.0".to_owned(),
                hash: "h".to_owned(),
            }
            .state_token(),
            "owned:dev.morphix.a"
        );
        assert_eq!(
            TargetOwnership::UnownedPresent { hash: None }.state_token(),
            "unowned"
        );
    }
}
