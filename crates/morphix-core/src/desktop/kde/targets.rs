//! Fixed KDE target map (P3-03, ARCHITECTURE.md §6).
//!
//! **Ownership.** The only place that knows KDE config file names, XDG asset locations,
//! and KDE DBus-adjacent target shapes. Packs name asset *keys*; this module resolves
//! them to engine-derived install locations and stable resource identities.
//!
//! This module is pure: it derives paths and identities from validated inputs and never
//! touches the filesystem, DBus, or storage. Pre-state for these targets is captured
//! through [`crate::planning::reads::PlannerReads`], which the planner owns.
//!
//! **Allowed dependencies:** `errors`, `core`.
//! **Must never depend on:** `packs`, `planning`, `transactions`, `storage`.
//!
//! # KDE facts encoded here
//!
//! Config file names (`kdeglobals`, `plasmarc`, `kwinrc`, `kglobalshortcutsrc`) and XDG
//! data locations are KDE knowledge, so they live here and nowhere else (AGENTS.md §5).
//! Appearance-managed keys follow PACK_SPEC.md §5.7 and the prototype results recorded in
//! ARCHITECTURE.md §9.2: the Plasma style selector is `plasmarc [Theme] name`, window
//! decoration state is `kwinrc [org.kde.kdecoration2]`, and cursors resolve to
//! `kcminputrc [Mouse]`.

use serde::{Deserialize, Serialize};

use crate::core::allowlist::ConfigTarget;
use crate::core::capability::Capability;
use crate::core::ids::{AssetKey, InstalledName, PackId};
use crate::core::resource::AssetKind;
use crate::errors::{ErrorCode, MorphixError, Result};

/// A typed config value shared by the target map, the planner, and (in Phase 6) the
/// adapter executors.
///
/// Values are read as raw strings from KConfig files; type coercion is validated before
/// writing (ARCHITECTURE.md §9.3). This type lives in `desktop` because only the adapter
/// interprets KConfig semantics; the planner treats it opaquely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum ConfigValue {
    /// A KConfig string value.
    String(String),
    /// A KConfig integer value.
    Integer(i64),
    /// A KConfig boolean value.
    Boolean(bool),
    /// A KConfig string-list value.
    Strings(Vec<String>),
}

impl ConfigValue {
    /// Renders the value the way KConfig stores it, for diffs and logs.
    ///
    /// String lists join with `,`; this is display-only, never a write encoding.
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Integer(value) => value.to_string(),
            Self::Boolean(value) => value.to_string(),
            Self::Strings(values) => values.join(","),
        }
    }
}

/// Observed pre-state of one resolved target.
///
/// `Absent` is recorded explicitly: restoring `Absent` means deleting the key or
/// removing the path, so inheritance from `kdedefaults` and `/etc/xdg` resumes
/// (TRANSACTIONS.md §6.2). It is never conflated with an empty string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ResourceState {
    /// The target does not exist; creating it is part of the plan.
    Absent,
    /// A config key's current value.
    ConfigValue(ConfigValue),
    /// A shortcut's current chord list.
    Shortcut {
        /// Currently bound chords, empty when unbound.
        keys: Vec<String>,
    },
    /// A KWin effect's current enabled state.
    Effect {
        /// Whether the effect is currently enabled.
        enabled: bool,
    },
    /// A wallpaper descriptor read through the independent read-back channel.
    Wallpaper {
        /// The current image descriptor (for example a `file://` URL).
        image: String,
    },
    /// An asset target already present on disk.
    AssetPresent {
        /// Tree or file hash of the existing target.
        hash: String,
    },
}

impl ResourceState {
    /// A short human-readable rendering for diffs. Never contains file contents.
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::Absent => "(absent)".to_owned(),
            Self::ConfigValue(value) => value.render(),
            Self::Shortcut { keys } => {
                if keys.is_empty() {
                    "(unbound)".to_owned()
                } else {
                    keys.join(" + ")
                }
            }
            Self::Effect { enabled } => {
                if *enabled {
                    "enabled".to_owned()
                } else {
                    "disabled".to_owned()
                }
            }
            Self::Wallpaper { image } => image.clone(),
            Self::AssetPresent { hash } => format!("(installed {hash})"),
        }
    }
}

/// The closed set of target kinds the V0.1 planner can resolve.
///
/// Panel opacity and cursor size are absent: no reliable mechanism was found, so packs
/// cannot request them (MVP.md §2.7, ARCHITECTURE.md §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// A single KConfig key.
    ConfigKey,
    /// A single installed asset file (color scheme, wallpaper).
    AssetFile,
    /// An installed asset directory tree (icons, cursors, plasma style, decorations).
    AssetDir,
    /// The current desktop wallpaper, set per desktop through scripting.
    Wallpaper,
    /// A global shortcut binding.
    Shortcut,
    /// A KWin effect toggle.
    KwinEffect,
    /// One panel property on one panel.
    PanelProperty,
    /// A whole panel (create/remove).
    Panel,
    /// A widget operation on a panel.
    Widget,
    /// A virtual-desktop operation.
    VirtualDesktop,
    /// The global-theme marker plus its decomposed components.
    GlobalTheme,
}

/// Where a config key lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigLocation {
    /// Which KDE config file holds the key.
    pub file: String,
    /// KConfig group name.
    pub group: String,
    /// KConfig key name.
    pub key: String,
}

/// An engine-resolved mutation target.
///
/// The identity is the stable conflict-scan key (DATABASE.md §6
/// `resource_identity`): two operations with the same identity touch the same
/// resource. The display is user-facing and never contains a host-absolute path
/// with the real home directory; absolute resolution happens at apply time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTarget {
    /// Stable identity for collision and conflict scans.
    pub identity: String,
    /// What kind of resource this is.
    pub kind: TargetKind,
    /// User-facing description, without host-absolute paths.
    pub display: String,
    /// Config location, when the target is a config key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<ConfigLocation>,
    /// Engine-relative asset path (for example `color-schemes/cupertino-dark.colors`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_path: Option<String>,
}

impl ResolvedTarget {
    /// Builds a config-key target with its canonical identity.
    #[must_use]
    pub fn config_key(file: ConfigTarget, group: &str, key: &str) -> Self {
        let file_name = file.as_str();
        Self {
            identity: format!("config:{file_name}:{group}:{key}"),
            kind: TargetKind::ConfigKey,
            display: format!("{file_name} [{group}] {key}"),
            config: Some(ConfigLocation {
                file: file_name.to_owned(),
                group: group.to_owned(),
                key: key.to_owned(),
            }),
            asset_path: None,
        }
    }

    /// Builds a target for a KDE config file outside the `[[settings]]` map
    /// (`plasmarc`, `kcminputrc`, `kglobalshortcutsrc`).
    #[must_use]
    pub fn named_config(file: &str, group: &str, key: &str) -> Self {
        Self {
            identity: format!("config:{file}:{group}:{key}"),
            kind: TargetKind::ConfigKey,
            display: format!("{file} [{group}] {key}"),
            config: Some(ConfigLocation {
                file: file.to_owned(),
                group: group.to_owned(),
                key: key.to_owned(),
            }),
            asset_path: None,
        }
    }
}

/// Derives the on-disk asset path (relative to its XDG root) for an installed asset.
///
/// Names are engine-derived (`<pack-slug>-<asset-key>`); packs cannot choose them
/// (ARCHITECTURE.md §6).
///
/// # Errors
///
/// Returns `E_INVALID_ID` when the installed name cannot be derived, and
/// `E_UNSUPPORTED_RESOURCE` for a wallpaper source path without a file extension.
pub fn installed_asset_path(
    pack_id: &PackId,
    kind: AssetKind,
    key: &AssetKey,
    source_path: Option<&str>,
) -> Result<String> {
    let installed = InstalledName::derive(pack_id, key)?;
    let name = installed.as_str();
    match kind {
        AssetKind::ColorScheme => Ok(format!("color-schemes/{name}.colors")),
        AssetKind::IconTheme | AssetKind::CursorTheme => Ok(format!("icons/{name}/")),
        AssetKind::PlasmaStyle => Ok(format!("plasma/desktoptheme/{name}/")),
        AssetKind::WindowDecoration => Ok(format!("aurorae/themes/{name}/")),
        AssetKind::Wallpaper => {
            let extension = source_path
                .and_then(|path| path.rsplit('.').next())
                .filter(|extension| {
                    ["png", "jpg", "jpeg", "webp"]
                        .contains(&extension.to_ascii_lowercase().as_str())
                })
                .ok_or_else(|| {
                    MorphixError::new(
                        ErrorCode::UnsupportedResource,
                        "a wallpaper asset needs a png, jpg, or webp extension",
                    )
                    .with_context("asset_key", key.as_str())
                })?;
            Ok(format!(
                "morphix/assets/wallpapers/{}/{name}.{extension}",
                pack_id.slug()
            ))
        }
    }
}

/// The XDG root a kind installs under, for display and ownership checks.
#[must_use]
pub fn asset_root(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::ColorScheme => "~/.local/share/color-schemes/",
        AssetKind::IconTheme | AssetKind::CursorTheme => "~/.local/share/icons/",
        AssetKind::PlasmaStyle => "~/.local/share/plasma/desktoptheme/",
        AssetKind::WindowDecoration => "~/.local/share/aurorae/themes/",
        AssetKind::Wallpaper => "~/.local/share/morphix/assets/wallpapers/",
    }
}

/// Resolves an asset target: identity plus display path.
#[must_use]
pub fn resolve_asset_target(
    pack_id: &PackId,
    kind: AssetKind,
    key: &AssetKey,
    relative_path: String,
) -> ResolvedTarget {
    let installed = InstalledName::derive(pack_id, key)
        .map(|name| name.as_str().to_owned())
        .unwrap_or_else(|_| format!("{}-{}", pack_id.slug(), key.as_str()));
    let display = format!("{}{relative_path}", asset_root(kind));
    let directory = kind.is_directory();
    ResolvedTarget {
        identity: format!("asset:{}:{installed}", kind.as_str()),
        kind: if directory {
            TargetKind::AssetDir
        } else {
            TargetKind::AssetFile
        },
        display,
        config: None,
        asset_path: Some(relative_path),
    }
}

/// Resolves a shortcut target.
#[must_use]
pub fn resolve_shortcut_target(component: &str, action: &str) -> ResolvedTarget {
    ResolvedTarget {
        identity: format!("shortcut:{component}:{action}"),
        kind: TargetKind::Shortcut,
        display: format!("shortcut {component} / {action}"),
        config: None,
        asset_path: None,
    }
}

/// Resolves a KWin effect target.
#[must_use]
pub fn resolve_effect_target(id: &str) -> ResolvedTarget {
    ResolvedTarget {
        identity: format!("effect:{id}"),
        kind: TargetKind::KwinEffect,
        display: format!("KWin effect {id}"),
        config: None,
        asset_path: None,
    }
}

/// Resolves the wallpaper target for one desktop scope.
///
/// V0.1 plans a single per-desktop entry (`all`); per-screen wallpaper is unverified
/// (MVP.md §1.1) and is not planned separately.
#[must_use]
pub fn resolve_wallpaper_target(pack_id: &PackId, key: &str) -> ResolvedTarget {
    ResolvedTarget {
        identity: format!("wallpaper:{}:{key}:all", pack_id.as_str()),
        kind: TargetKind::Wallpaper,
        display: "desktop wallpaper".to_owned(),
        config: None,
        asset_path: None,
    }
}

/// Resolves a panel-property target.
#[must_use]
pub fn resolve_panel_property_target(panel_id: u32, property: &str) -> ResolvedTarget {
    ResolvedTarget {
        identity: format!("panel:{panel_id}:{property}"),
        kind: TargetKind::PanelProperty,
        display: format!("panel {panel_id} {property}"),
        config: None,
        asset_path: None,
    }
}

/// Resolves a whole-panel target (create/remove).
#[must_use]
pub fn resolve_panel_target(panel_id: u32) -> ResolvedTarget {
    ResolvedTarget {
        identity: format!("panel:{panel_id}"),
        kind: TargetKind::Panel,
        display: format!("panel {panel_id}"),
        config: None,
        asset_path: None,
    }
}

/// Resolves a widget-operation target.
#[must_use]
pub fn resolve_widget_target(panel_id: u32, widget_type: &str, index: i32) -> ResolvedTarget {
    ResolvedTarget {
        identity: format!("widget:{panel_id}:{widget_type}:{index}"),
        kind: TargetKind::Widget,
        display: format!("panel {panel_id} widget {widget_type}"),
        config: None,
        asset_path: None,
    }
}

/// Resolves a virtual-desktop target.
#[must_use]
pub fn resolve_virtual_desktop_target(action: &str, name: Option<&str>) -> ResolvedTarget {
    let identity = match name {
        Some(name) => format!("virtual-desktop:{action}:{name}"),
        None => format!("virtual-desktop:{action}"),
    };
    ResolvedTarget {
        identity,
        kind: TargetKind::VirtualDesktop,
        display: match name {
            Some(name) => format!("virtual desktop {action} {name}"),
            None => format!("virtual desktop {action}"),
        },
        config: None,
        asset_path: None,
    }
}

/// Resolves the global-theme marker target.
#[must_use]
pub fn resolve_global_theme_target(package_id: &str) -> ResolvedTarget {
    ResolvedTarget {
        identity: format!("global-theme:{package_id}"),
        kind: TargetKind::GlobalTheme,
        display: format!("global theme {package_id}"),
        config: Some(ConfigLocation {
            file: "kdeglobals".to_owned(),
            group: "KDE".to_owned(),
            key: "LookAndFeelPackage".to_owned(),
        }),
        asset_path: None,
    }
}

/// The capability that owns each appearance selection.
///
/// Appearance-managed keys are engine-owned and unreachable through `[[settings]]`
/// (PACK_SPEC.md §5.7); the planner uses this table so a pack cannot bypass the
/// capability check by spelling the same key as a setting.
#[must_use]
pub fn appearance_capability(field: &str) -> Option<Capability> {
    match field {
        "color_scheme" => Some(Capability::ColorScheme),
        "icon_theme" => Some(Capability::Icons),
        "cursor_theme" | "cursor_size" => Some(Capability::Cursors),
        "plasma_style" => Some(Capability::PlasmaStyle),
        "window_decoration" => Some(Capability::WindowDecorations),
        "wallpaper" => Some(Capability::Wallpaper),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        ConfigValue, ResolvedTarget, ResourceState, appearance_capability, asset_root,
        installed_asset_path, resolve_asset_target, resolve_effect_target,
        resolve_panel_property_target, resolve_shortcut_target, resolve_virtual_desktop_target,
        resolve_wallpaper_target,
    };
    use crate::core::capability::Capability;
    use crate::core::ids::{AssetKey, PackId};
    use crate::core::resource::AssetKind;

    fn pack() -> PackId {
        PackId::parse("dev.morphix.cupertino").expect("valid pack id")
    }

    fn key(value: &str) -> AssetKey {
        AssetKey::parse(value).expect("valid asset key")
    }

    #[test]
    fn installed_names_are_engine_derived_per_kind() {
        let pack = pack();
        let colors = installed_asset_path(&pack, AssetKind::ColorScheme, &key("dark"), None)
            .expect("colors path");
        assert_eq!(colors, "color-schemes/cupertino-dark.colors");

        let wallpaper = installed_asset_path(
            &pack,
            AssetKind::Wallpaper,
            &key("dark"),
            Some("wallpapers/dark.webp"),
        )
        .expect("wallpaper path");
        assert_eq!(
            wallpaper,
            "morphix/assets/wallpapers/cupertino/cupertino-dark.webp"
        );

        assert!(asset_root(AssetKind::Wallpaper).contains("morphix"));
    }

    #[test]
    fn wallpaper_without_an_image_extension_is_refused() {
        let error =
            installed_asset_path(&pack(), AssetKind::Wallpaper, &key("dark"), Some("w/bmp"))
                .expect_err("bmp is not a wallpaper");
        assert_eq!(error.code, crate::errors::ErrorCode::UnsupportedResource);
    }

    #[test]
    fn identities_are_stable_and_unique_per_resource() {
        let pack = pack();
        let first =
            resolve_asset_target(&pack, AssetKind::ColorScheme, &key("dark"), String::new());
        let second =
            resolve_asset_target(&pack, AssetKind::ColorScheme, &key("dark"), String::new());
        assert_eq!(first.identity, second.identity);

        let other =
            resolve_asset_target(&pack, AssetKind::ColorScheme, &key("light"), String::new());
        assert_ne!(first.identity, other.identity);

        assert_ne!(
            resolve_shortcut_target("kwin", "Show Desktop").identity,
            resolve_effect_target("blur").identity
        );
        assert_ne!(
            resolve_wallpaper_target(&pack, "dark").identity,
            resolve_wallpaper_target(&pack, "light").identity
        );
        assert_ne!(
            resolve_panel_property_target(0, "height").identity,
            resolve_panel_property_target(1, "height").identity
        );
        assert_ne!(
            resolve_virtual_desktop_target("add", Some("Work")).identity,
            resolve_virtual_desktop_target("remove", Some("Work")).identity
        );
    }

    #[test]
    fn absent_is_distinct_from_empty_and_renders_safely() {
        assert_eq!(ResourceState::Absent.render(), "(absent)");
        assert_eq!(
            ResourceState::ConfigValue(ConfigValue::String(String::new())).render(),
            ""
        );
        assert_eq!(
            ResourceState::Shortcut { keys: Vec::new() }.render(),
            "(unbound)"
        );
    }

    #[test]
    fn appearance_fields_map_to_their_owning_capability() {
        assert_eq!(
            appearance_capability("color_scheme"),
            Some(Capability::ColorScheme)
        );
        assert_eq!(
            appearance_capability("cursor_size"),
            Some(Capability::Cursors)
        );
        assert_eq!(appearance_capability("unknown"), None);
    }

    #[test]
    fn config_targets_carry_their_location() {
        let target = ResolvedTarget::config_key(
            crate::core::allowlist::ConfigTarget::Kdeglobals,
            "General",
            "font",
        );
        assert_eq!(target.identity, "config:kdeglobals:General:font");
        assert!(target.config.is_some());
    }
}
