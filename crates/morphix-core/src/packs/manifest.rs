//! Typed, strict manifest models for schema v1 (PACK_SPEC.md §4–§6.1).
//!
//! Every struct sets `deny_unknown_fields`, so an unrecognized or reserved TOML key fails
//! deserialization instead of being silently ignored (PACK_SPEC.md §1 rule 5). Field
//! *values* are deliberately kept narrow (`Option<i64>`, `String`) so semantic ranges are
//! checked by [`crate::packs::validator`] and reported together with other issues; the
//! shape itself is enforced here.
//!
//! Raw manifest text is visible only to [`crate::packs::parser`]. Everything downstream
//! consumes these typed models (SECURITY.md §4).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::core::capability::Capability;
use crate::core::ids::{AssetKey, PackId};
use crate::core::resource::AssetKind;
use crate::core::versions::PackVersion;

/// The only schema version understood by V0.1.
pub const SCHEMA_VERSION_V1: u32 = 1;

/// The complete, strictly-typed schema-v1 manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Must be exactly [`SCHEMA_VERSION_V1`].
    pub schema_version: u32,
    /// Identity and human-facing metadata.
    pub pack: PackInfo,
    /// Desktop and Plasma range.
    pub compatibility: Compatibility,
    /// The closed capability request.
    pub capabilities: Capabilities,
    /// Declared installable assets. Empty only when no asset is requested.
    #[serde(default)]
    pub assets: Assets,
    /// Convenience selection of declared assets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
    /// Allowlisted configuration writes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<SettingEntry>,
    /// KWin effect toggles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kwin: Option<KwinSection>,
    /// Allowlisted global shortcuts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shortcuts: Vec<ShortcutEntry>,
    /// Display-only screenshots. One to eight are required.
    pub previews: Vec<PreviewEntry>,
    /// Optional author-provided checksums.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<IntegritySection>,
    /// Declared pack dependencies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Dependencies>,
    /// Panel declarations (PACK_SPEC.md §6.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<Panel>,
    /// Virtual-desktop and KWin behaviour declarations (PACK_SPEC.md §6.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_behavior: Option<DesktopBehavior>,
}

impl Manifest {
    /// The declared capability set, parsed by the parser.
    #[must_use]
    pub fn requested_capabilities(&self) -> Vec<Capability> {
        self.capabilities
            .requires
            .iter()
            .filter_map(|id| Capability::from_id(id))
            .collect()
    }

    /// Declares a capability by its exact ID.
    #[must_use]
    pub fn requests(&self, capability: Capability) -> bool {
        self.capabilities
            .requires
            .iter()
            .any(|id| id == capability.as_str())
    }

    /// Iterates declared assets with their kind and validated key.
    pub fn assets_iter(&self) -> impl Iterator<Item = (AssetKind, &AssetKey, &str)> {
        AssetKind::ALL.into_iter().flat_map(move |kind| {
            self.assets
                .for_kind(kind)
                .iter()
                .map(move |(key, path)| (kind, key, path.as_str()))
        })
    }
}

/// `[pack]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackInfo {
    /// Reverse-DNS identity.
    pub id: PackId,
    /// Display name, 1..=80 characters.
    pub name: String,
    /// Strict SemVer version.
    pub version: PackVersion,
    /// One-line summary, 1..=200 characters.
    pub summary: String,
    /// Markdown description, 1..=8192 characters.
    pub description: String,
    /// Authors, 1..=16 entries.
    pub authors: Vec<String>,
    /// SPDX expression.
    pub license: String,
    /// Optional display-only HTTPS URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Icon path under `previews/`.
    pub icon: String,
}

/// `[compatibility]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    /// Exactly `kde` in schema v1.
    pub desktop: String,
    /// SemVer range for KDE Plasma.
    pub plasma: String,
}

/// `[capabilities]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// Exact capability IDs. Kept as strings so unknown IDs are reported, not dropped.
    pub requires: Vec<String>,
}

/// `[assets]` with one optional table per kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assets {
    /// `.colors` files.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub color_schemes: BTreeMap<AssetKey, String>,
    /// Icon-theme directories.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub icon_themes: BTreeMap<AssetKey, String>,
    /// Cursor-theme directories.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cursor_themes: BTreeMap<AssetKey, String>,
    /// Plasma-style directories.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plasma_styles: BTreeMap<AssetKey, String>,
    /// Aurorae window-decoration directories.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub window_decorations: BTreeMap<AssetKey, String>,
    /// Wallpaper images.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub wallpapers: BTreeMap<AssetKey, String>,
}

impl Assets {
    /// The map for one kind.
    #[must_use]
    pub fn for_kind(&self, kind: AssetKind) -> &BTreeMap<AssetKey, String> {
        match kind {
            AssetKind::ColorScheme => &self.color_schemes,
            AssetKind::IconTheme => &self.icon_themes,
            AssetKind::CursorTheme => &self.cursor_themes,
            AssetKind::PlasmaStyle => &self.plasma_styles,
            AssetKind::WindowDecoration => &self.window_decorations,
            AssetKind::Wallpaper => &self.wallpapers,
        }
    }

    /// Whether no asset is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        AssetKind::ALL
            .into_iter()
            .all(|kind| self.for_kind(kind).is_empty())
    }
}

/// `[appearance]`, including the `[appearance.look_and_feel]` form.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    /// Key into `assets.color_schemes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<String>,
    /// Key into `assets.icon_themes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_theme: Option<String>,
    /// Key into `assets.cursor_themes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_theme: Option<String>,
    /// 16..=128; requires `cursor_theme`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_size: Option<i64>,
    /// Key into `assets.plasma_styles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plasma_style: Option<String>,
    /// Key into `assets.window_decorations`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_decoration: Option<String>,
    /// Key into `assets.wallpapers`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<String>,
    /// Global-theme composition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look_and_feel: Option<LookAndFeel>,
}

impl Appearance {
    /// Whether every field is absent.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.color_scheme.is_none()
            && self.icon_theme.is_none()
            && self.cursor_theme.is_none()
            && self.cursor_size.is_none()
            && self.plasma_style.is_none()
            && self.window_decoration.is_none()
            && self.wallpaper.is_none()
            && self.look_and_feel.is_none()
    }
}

/// `[appearance.look_and_feel]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookAndFeel {
    /// Installed Plasma look-and-feel package ID.
    pub package: String,
    /// Primitive components to compose.
    pub components: Vec<String>,
}

/// One `[[settings]]` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingEntry {
    /// `kdeglobals` or `kwinrc`.
    pub target: String,
    /// KConfig group name.
    pub group: String,
    /// KConfig key name.
    pub key: String,
    /// Typed allowlisted value.
    pub value: SettingValue,
}

/// A value a `[[settings]]` row may carry (PACK_SPEC.md §5.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingValue {
    /// A boolean KConfig value.
    Boolean(bool),
    /// An integer KConfig value.
    Integer(i64),
    /// A string KConfig value.
    String(String),
    /// A string-list KConfig value.
    Strings(Vec<String>),
}

impl SettingValue {
    /// Stable type name used by the allowlist and diagnostics.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Boolean(_) => "boolean",
            Self::Integer(_) => "integer",
            Self::String(_) => "string",
            Self::Strings(_) => "string_list",
        }
    }

    /// The scalar string form, if this is a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }
}

/// `[kwin]` with its `[[kwin.effects]]` rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KwinSection {
    /// Effect toggles, at most 32.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<KwinEffect>,
}

/// One `[[kwin.effects]]` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KwinEffect {
    /// Allowlisted effect ID.
    pub id: String,
    /// Desired enabled state.
    pub enabled: bool,
}

/// One `[[shortcuts]]` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortcutEntry {
    /// `kwin` or `plasmashell`.
    pub component: String,
    /// Allowlisted action name.
    pub action: String,
    /// One to four unique chords.
    pub keys: Vec<String>,
}

/// One `[[previews]]` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewEntry {
    /// Path under `previews/`.
    pub image: String,
    /// 1..=120 characters.
    pub caption: String,
}

/// `[integrity]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegritySection {
    /// Exactly `sha256`.
    pub algorithm: String,
    /// Exactly `checksums.sha256`.
    pub manifest: String,
}

/// `[dependencies]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependencies {
    /// Required packs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packs: Vec<Dependency>,
}

/// One `[[dependencies.packs]]` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    /// Another pack's reverse-DNS ID.
    pub id: String,
    /// SemVer range the installed version must satisfy.
    pub version: String,
}

/// `[panel]` plus `[[panel.properties]]` and `[[panel.widgets]]` (PACK_SPEC.md §6.1).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    /// Whether the pack creates a panel.
    #[serde(default)]
    pub create: bool,
    /// Property changes on existing panels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<PanelProperty>,
    /// Widget operations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widgets: Vec<PanelWidget>,
}

/// One `[[panel.properties]]` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelProperty {
    /// `top`, `bottom`, `left`, or `right`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// 16..=128.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    /// `left`, `center`, or `right`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<String>,
    /// Whether the panel floats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floating: Option<bool>,
    /// `none`, `autohide`, or `dodgewindows`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hiding: Option<String>,
}

impl PanelProperty {
    /// Whether the row changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.location.is_none()
            && self.height.is_none()
            && self.alignment.is_none()
            && self.floating.is_none()
            && self.hiding.is_none()
    }
}

/// One `[[panel.widgets]]` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelWidget {
    /// Index into the pack's panel list (or an existing panel ID).
    pub panel: i64,
    /// `add`, `configure`, or `remove`.
    pub op: String,
    /// Curated widget type.
    #[serde(rename = "type")]
    pub widget_type: String,
    /// Must be `true`; kept to make the safety declaration explicit.
    pub safe: bool,
}

/// `[desktop_behavior]` with its `[[desktop_behavior.virtual_desktops]]` rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopBehavior {
    /// Virtual-desktop operations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub virtual_desktops: Vec<VirtualDesktop>,
}

/// One `[[desktop_behavior.virtual_desktops]]` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VirtualDesktop {
    /// `add`, `rename`, or `remove`.
    pub op: String,
    /// Desktop name for `add`/`rename`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{Manifest, SettingValue};

    const MINIMAL: &str = r#"
schema_version = 1

[pack]
id = "dev.example.minimal"
name = "Minimal"
version = "0.1.0"
summary = "A minimal pack."
description = "A minimal pack used by the model tests."
authors = ["Tester"]
license = "MIT"
icon = "previews/icon.png"

[compatibility]
desktop = "kde"
plasma = ">=6.0, <7.0"

[capabilities]
requires = []

[assets]

[[previews]]
image = "previews/desktop.png"
caption = "Desktop"
"#;

    #[test]
    fn a_minimal_manifest_deserializes() {
        let manifest: Manifest = toml::from_str(MINIMAL).expect("minimal manifest");
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.pack.id.as_str(), "dev.example.minimal");
        assert!(manifest.assets.is_empty());
        assert!(manifest.appearance.is_none());
    }

    #[test]
    fn unknown_fields_are_refused_at_every_level() {
        let unknown = MINIMAL.replace("name = \"Minimal\"", "name = \"Minimal\"\ncolour = \"red\"");
        assert!(toml::from_str::<Manifest>(&unknown).is_err());

        let reserved = format!("{MINIMAL}\n[scripts]\nrun = \"true\"\n");
        assert!(toml::from_str::<Manifest>(&reserved).is_err());
    }

    #[test]
    fn setting_values_keep_their_declared_type() {
        let source = format!(
            "{MINIMAL}\n[[settings]]\ntarget = \"kdeglobals\"\ngroup = \"General\"\nkey = \"font\"\nvalue = \"Noto Sans,10\"\n"
        );
        let manifest: Manifest = toml::from_str(&source).expect("settings manifest");
        assert_eq!(
            manifest.settings[0].value,
            SettingValue::String("Noto Sans,10".to_owned())
        );
    }
}
