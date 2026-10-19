//! The closed V0.1 capability set.
//!
//! **Ownership.** The fifteen capability identifiers a pack may request and an adapter may
//! report support for (MVP.md §4, PACK_SPEC.md §6). Types only: no I/O and no validation
//! policy beyond the closed set itself.
//!
//! The set is closed. `panel.opacity` and cursor size are deliberately absent; a pack that
//! requests them must fail with `E_CAPABILITY_UNKNOWN`, never be downgraded to a no-op.
//! Adding a capability requires an MVP/PACK_SPEC change and a new schema version.
//!
//! **Task note.** P2-07 owns this enum in the phase plan, but `AdapterProbe` needs the
//! closed set in Phase 1, so it lives here now; P2-07 consumes it for manifest validation.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A capability a schema-v1 pack may request.
///
/// Serde names are the exact dotted IDs from PACK_SPEC.md §6; they are stable IPC and
/// manifest values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// Install and select one color scheme.
    #[serde(rename = "appearance.color_scheme")]
    ColorScheme,
    /// Install and select one icon theme.
    #[serde(rename = "appearance.icons")]
    Icons,
    /// Install and select one cursor theme.
    #[serde(rename = "appearance.cursors")]
    Cursors,
    /// Install and select one Plasma style.
    #[serde(rename = "appearance.plasma_style")]
    PlasmaStyle,
    /// Install and select one window decoration.
    #[serde(rename = "appearance.window_decorations")]
    WindowDecorations,
    /// Install one wallpaper and set the current background.
    #[serde(rename = "appearance.wallpaper")]
    Wallpaper,
    /// Install and apply one global theme, decomposed into primitive operations.
    #[serde(rename = "appearance.look_and_feel")]
    LookAndFeel,
    /// Write allowlisted font keys.
    #[serde(rename = "appearance.fonts")]
    Fonts,
    /// Set edge, height, alignment, floating and hiding on existing panels.
    #[serde(rename = "panel.properties")]
    PanelProperties,
    /// Add, configure and remove allowlisted widgets.
    #[serde(rename = "panel.widgets")]
    PanelWidgets,
    /// Create and remove a panel.
    #[serde(rename = "panel.create")]
    PanelCreate,
    /// Write allowlisted `kdeglobals` keys.
    #[serde(rename = "settings.kde_global")]
    KdeGlobal,
    /// Toggle and configure allowlisted KWin effects.
    #[serde(rename = "settings.kwin_effects")]
    KwinEffects,
    /// Set allowlisted global shortcuts.
    #[serde(rename = "settings.shortcuts")]
    Shortcuts,
    /// Change allowlisted KWin behaviour keys and virtual desktops.
    #[serde(rename = "settings.desktop_behavior")]
    DesktopBehavior,
}

impl Capability {
    /// Every capability, in documentation order.
    pub const ALL: [Capability; 15] = [
        Self::ColorScheme,
        Self::Icons,
        Self::Cursors,
        Self::PlasmaStyle,
        Self::WindowDecorations,
        Self::Wallpaper,
        Self::LookAndFeel,
        Self::Fonts,
        Self::PanelProperties,
        Self::PanelWidgets,
        Self::PanelCreate,
        Self::KdeGlobal,
        Self::KwinEffects,
        Self::Shortcuts,
        Self::DesktopBehavior,
    ];

    /// The exact manifest/IPC identifier.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ColorScheme => "appearance.color_scheme",
            Self::Icons => "appearance.icons",
            Self::Cursors => "appearance.cursors",
            Self::PlasmaStyle => "appearance.plasma_style",
            Self::WindowDecorations => "appearance.window_decorations",
            Self::Wallpaper => "appearance.wallpaper",
            Self::LookAndFeel => "appearance.look_and_feel",
            Self::Fonts => "appearance.fonts",
            Self::PanelProperties => "panel.properties",
            Self::PanelWidgets => "panel.widgets",
            Self::PanelCreate => "panel.create",
            Self::KdeGlobal => "settings.kde_global",
            Self::KwinEffects => "settings.kwin_effects",
            Self::Shortcuts => "settings.shortcuts",
            Self::DesktopBehavior => "settings.desktop_behavior",
        }
    }

    /// Looks up a capability by its exact ID. Case and whitespace are not normalized:
    /// manifest values are strict (PACK_SPEC.md §6).
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|capability| capability.as_str() == id)
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::collections::BTreeSet;

    use super::Capability;

    #[test]
    fn the_set_is_closed_at_fifteen() {
        assert_eq!(Capability::ALL.len(), 15);

        let unique: BTreeSet<&str> = Capability::ALL
            .iter()
            .map(|capability| capability.as_str())
            .collect();
        assert_eq!(unique.len(), 15, "capability IDs must be unique");
    }

    #[test]
    fn ids_match_the_manifest_table() {
        let expected = [
            "appearance.color_scheme",
            "appearance.icons",
            "appearance.cursors",
            "appearance.plasma_style",
            "appearance.window_decorations",
            "appearance.wallpaper",
            "appearance.look_and_feel",
            "appearance.fonts",
            "panel.properties",
            "panel.widgets",
            "panel.create",
            "settings.kde_global",
            "settings.kwin_effects",
            "settings.shortcuts",
            "settings.desktop_behavior",
        ];

        for (capability, id) in Capability::ALL.into_iter().zip(expected) {
            assert_eq!(capability.as_str(), id);
            assert_eq!(capability.to_string(), id);
            assert_eq!(Capability::from_id(id), Some(capability));
        }
    }

    #[test]
    fn from_id_rejects_unknown_and_non_exact_ids() {
        for id in [
            "",
            "panel.opacity",
            "cursor_size",
            "appearance.Color_Scheme",
            " appearance.color_scheme",
            "appearance.color_scheme ",
            "appearance.color-scheme",
        ] {
            assert_eq!(Capability::from_id(id), None, "id: {id:?}");
        }
    }

    #[test]
    fn serde_uses_the_exact_dotted_ids() {
        for capability in Capability::ALL {
            let json = serde_json::to_string(&capability).expect("serializes");
            assert_eq!(json, format!("\"{}\"", capability.as_str()));

            let back: Capability = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(back, capability);
        }
    }

    #[test]
    fn serde_rejects_a_capability_outside_the_set() {
        let error = serde_json::from_str::<Capability>("\"panel.opacity\"")
            .expect_err("unknown capability is rejected");
        assert!(!error.to_string().is_empty());
    }
}
