//! Resource kinds shared by the pack, planning, and storage layers.
//!
//! **Ownership.** Closed enums for the six installable asset kinds. No I/O and no KDE path
//! knowledge: the target map belongs to `desktop::kde::targets` (Phase 3/6).

use serde::{Deserialize, Serialize};

use crate::core::ids::AssetKey;
use crate::core::ids::InstalledName;

/// An installable asset kind in schema v1 (PACK_SPEC.md §5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// A KDE `.colors` file.
    ColorScheme,
    /// An icon theme directory.
    IconTheme,
    /// A cursor theme directory containing `cursors/`.
    CursorTheme,
    /// A Plasma style (desktop theme) directory.
    PlasmaStyle,
    /// An Aurorae window-decoration directory.
    WindowDecoration,
    /// A wallpaper image.
    Wallpaper,
}

impl AssetKind {
    /// Every kind, in PACK_SPEC.md order.
    pub const ALL: [AssetKind; 6] = [
        Self::ColorScheme,
        Self::IconTheme,
        Self::CursorTheme,
        Self::PlasmaStyle,
        Self::WindowDecoration,
        Self::Wallpaper,
    ];

    /// The stable identifier used in the database and IPC (`color_scheme`, …).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ColorScheme => "color_scheme",
            Self::IconTheme => "icon_theme",
            Self::CursorTheme => "cursor_theme",
            Self::PlasmaStyle => "plasma_style",
            Self::WindowDecoration => "window_decoration",
            Self::Wallpaper => "wallpaper",
        }
    }

    /// The manifest table name under `[assets]` (`color_schemes`, …).
    #[must_use]
    pub fn manifest_table(self) -> &'static str {
        match self {
            Self::ColorScheme => "color_schemes",
            Self::IconTheme => "icon_themes",
            Self::CursorTheme => "cursor_themes",
            Self::PlasmaStyle => "plasma_styles",
            Self::WindowDecoration => "window_decorations",
            Self::Wallpaper => "wallpapers",
        }
    }

    /// Whether the asset is installed as a directory tree.
    #[must_use]
    pub fn is_directory(self) -> bool {
        matches!(
            self,
            Self::IconTheme | Self::CursorTheme | Self::PlasmaStyle | Self::WindowDecoration
        )
    }

    /// Resolves a manifest table name.
    #[must_use]
    pub fn from_manifest_table(table: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.manifest_table() == table)
    }
}

/// Where an available pack came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackSource {
    /// Shipped with the application.
    Bundled,
    /// Installed by the user from a local directory.
    Local,
}

impl PackSource {
    /// The database spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::Local => "local",
        }
    }
}

/// Whether a registered pack currently passes validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackStatus {
    /// Validation passed.
    Valid,
    /// Validation failed; the pack is listed read-only with its issues.
    Invalid,
}

impl PackStatus {
    /// The database spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
        }
    }
}

/// Where a pack's integrity hash came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegritySource {
    /// An author-provided `checksums.sha256` verified successfully.
    Declared,
    /// Morphix computed the content hash at install time; the pack is **Unverified**.
    GeneratedLocal,
}

impl IntegritySource {
    /// The database spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Declared => "declared",
            Self::GeneratedLocal => "generated_local",
        }
    }
}

/// Enough information to name one declared asset in a catalog or plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRef {
    /// The asset kind.
    pub kind: AssetKind,
    /// The manifest key.
    pub key: AssetKey,
    /// The validated pack-relative path.
    pub path: String,
    /// The engine-derived on-disk name.
    pub installed_name: InstalledName,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::AssetKind;

    #[test]
    fn identifiers_and_tables_match_the_specification() {
        let expected = [
            ("color_scheme", "color_schemes", false),
            ("icon_theme", "icon_themes", true),
            ("cursor_theme", "cursor_themes", true),
            ("plasma_style", "plasma_styles", true),
            ("window_decoration", "window_decorations", true),
            ("wallpaper", "wallpapers", false),
        ];

        for (kind, (id, table, directory)) in AssetKind::ALL.into_iter().zip(expected) {
            assert_eq!(kind.as_str(), id);
            assert_eq!(kind.manifest_table(), table);
            assert_eq!(kind.is_directory(), directory);
            assert_eq!(AssetKind::from_manifest_table(table), Some(kind));
        }
        assert_eq!(AssetKind::from_manifest_table("unknown"), None);
    }
}
