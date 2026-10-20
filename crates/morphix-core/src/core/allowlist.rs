//! Code-owned allowlists for pack-declared changes (P2-08).
//!
//! **Ownership.** The closed sets a schema-v1 pack may target: KDE config files and keys,
//! KWin effects, shortcut components/actions, panel properties, widget types, and
//! look-and-feel components. The tables are pure data with no I/O.
//!
//! **Why this is in `core`.** `packs::validator` must check declarations against the same
//! tables that `permissions` grants from, and the architecture forbids `packs` from
//! depending on `permissions` (ARCHITECTURE.md §5). `core` is the only common dependency,
//! so the tables live here; `permissions::allowlist` owns the grant-facing API.
//!
//! These lists are deliberately small. A key, effect, or widget type that is not proven to
//! work is rejected rather than allowed silently (SECURITY.md T-34, PACK_SPEC.md §5.7).

use crate::core::capability::Capability;

/// Config files a `[[settings]]` entry may target (PACK_SPEC.md §5.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfigTarget {
    /// `~/.config/kdeglobals`.
    Kdeglobals,
    /// `~/.config/kwinrc`.
    Kwinrc,
}

impl ConfigTarget {
    /// Every allowed target.
    pub const ALL: [ConfigTarget; 2] = [Self::Kdeglobals, Self::Kwinrc];

    /// Parses the exact manifest target name.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "kdeglobals" => Some(Self::Kdeglobals),
            "kwinrc" => Some(Self::Kwinrc),
            _ => None,
        }
    }

    /// The manifest/database spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kdeglobals => "kdeglobals",
            Self::Kwinrc => "kwinrc",
        }
    }
}

/// The value type an allowlisted key accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigValueType {
    /// A single string (fonts and similar).
    String,
    /// A signed integer.
    Integer,
    /// A boolean.
    Boolean,
    /// An array of strings.
    StringArray,
}

impl ConfigValueType {
    /// Human-readable name for validation messages.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::StringArray => "array of strings",
        }
    }
}

/// The `kdeglobals [General]` font keys.
pub const FONT_KEYS: [&str; 6] = [
    "font",
    "fixed",
    "fixedWidthFont",
    "menuFont",
    "toolBarFont",
    "smallestReadableFont",
];

/// Whether a `kdeglobals [General]` key is a font key.
#[must_use]
pub fn is_font_key(key: &str) -> bool {
    FONT_KEYS.contains(&key)
}

/// The initial KWin effect allowlist (PACK_SPEC.md §5.8).
pub const KWIN_EFFECTS: [&str; 9] = [
    "blur",
    "contrast",
    "kwin4_effect_dimscreen",
    "kwin4_effect_fadingpopups",
    "kwin4_effect_scale",
    "magiclamp",
    "slidingpopups",
    "wobblywindows",
    "zoom",
];

/// Whether an effect ID is on the allowlist.
#[must_use]
pub fn is_kwin_effect(id: &str) -> bool {
    KWIN_EFFECTS.contains(&id)
}

/// Effect-specific config keys.
///
/// The list is intentionally empty in Phase 2: a key is only added after Phase 6 proves the
/// effect reads it back. Toggling `<id>Enabled` remains available. An empty list means
/// "not configurable yet", not "anything is allowed".
const EFFECT_CONFIG_KEYS: &[(&str, &[(&str, ConfigValueType)])] = &[
    ("blur", &[]),
    ("contrast", &[]),
    ("kwin4_effect_dimscreen", &[]),
    ("kwin4_effect_fadingpopups", &[]),
    ("kwin4_effect_scale", &[]),
    ("magiclamp", &[]),
    ("slidingpopups", &[]),
    ("wobblywindows", &[]),
    ("zoom", &[]),
];

/// Returns the accepted value type for a config key, or `None` when it is not allowlisted.
///
/// Engine-owned appearance keys return `None`: they are reachable only through
/// `[appearance]`, never through `[[settings]]`.
#[must_use]
pub fn config_key_type(target: ConfigTarget, group: &str, key: &str) -> Option<ConfigValueType> {
    if is_engine_owned_config(target, group, key) {
        return None;
    }

    match target {
        ConfigTarget::Kdeglobals => match group {
            "General" if is_font_key(key) => Some(ConfigValueType::String),
            _ => None,
        },
        ConfigTarget::Kwinrc => {
            if group == "Plugins" {
                let effect = key.strip_suffix("Enabled")?;
                return is_kwin_effect(effect).then_some(ConfigValueType::Boolean);
            }
            let effect = group.strip_prefix("Effect-")?;
            if !is_kwin_effect(effect) {
                return None;
            }
            EFFECT_CONFIG_KEYS
                .iter()
                .find(|(id, _)| *id == effect)
                .and_then(|(_, keys)| {
                    keys.iter()
                        .find(|(candidate, _)| *candidate == key)
                        .map(|(_, value_type)| *value_type)
                })
        }
    }
}

/// Whether a config key is engine-owned by an `[appearance]` field.
#[must_use]
pub fn is_engine_owned_config(target: ConfigTarget, group: &str, key: &str) -> bool {
    matches!(
        (target, group, key),
        (
            ConfigTarget::Kdeglobals,
            "General",
            "ColorScheme" | "cursorTheme" | "cursorSize"
        ) | (ConfigTarget::Kdeglobals, "Icons", "Theme")
            | (
                ConfigTarget::Kdeglobals,
                "org.kde.kdecoration2",
                "theme" | "library"
            )
    )
}

/// The capability or capabilities that own a config key.
///
/// A font key may be claimed by either `appearance.fonts` or `settings.kde_global`; the
/// validator accepts either declaration and marks every declared owner as used. This is
/// the one overlap in the capability table (PACK_SPEC.md §5.7).
#[must_use]
pub fn config_key_capabilities(
    target: ConfigTarget,
    group: &str,
    key: &str,
) -> &'static [Capability] {
    const FONT: &[Capability] = &[Capability::Fonts, Capability::KdeGlobal];
    const GLOBAL: &[Capability] = &[Capability::KdeGlobal];
    const EFFECTS: &[Capability] = &[Capability::KwinEffects];

    match target {
        ConfigTarget::Kdeglobals if group == "General" && is_font_key(key) => FONT,
        ConfigTarget::Kdeglobals => GLOBAL,
        ConfigTarget::Kwinrc => EFFECTS,
    }
}

/// Shortcut components allowed in schema v1 (PACK_SPEC.md §5.9).
pub const SHORTCUT_COMPONENTS: [&str; 2] = ["kwin", "plasmashell"];

/// Actions allowed per shortcut component.
///
/// These are KDE's own action names (the spelling `kglobalaccel` matches). Phase 6 checks
/// availability at runtime; an action that is not installed becomes a plan incompatibility
/// rather than a silent no-op.
const SHORTCUT_ACTIONS: &[(&str, &[&str])] = &[
    (
        "kwin",
        &[
            "Show Desktop",
            "Expose",
            "Walk Through Windows",
            "Walk Through Windows (Reverse)",
            "Window Close",
            "Window Maximize",
            "Window Minimize",
        ],
    ),
    (
        "plasmashell",
        &["Activate Application Launcher", "Show Dashboard"],
    ),
];

/// Whether a shortcut component/action pair is allowlisted.
#[must_use]
pub fn is_shortcut_allowed(component: &str, action: &str) -> bool {
    SHORTCUT_ACTIONS
        .iter()
        .find(|(candidate, _)| *candidate == component)
        .is_some_and(|(_, actions)| actions.contains(&action))
}

/// Maximum number of chords per shortcut.
pub const MAX_SHORTCUT_CHORDS: usize = 4;

/// Modifiers allowed in a shortcut chord.
pub const SHORTCUT_MODIFIERS: [&str; 4] = ["Meta", "Ctrl", "Alt", "Shift"];

/// Validates a single chord such as `Meta+D`.
///
/// The grammar is one to three allowlisted modifiers followed by one alphanumeric key,
/// separated by `+`; a bare key is accepted because PACK_SPEC.md §5.9's pattern allows it.
#[must_use]
pub fn is_valid_shortcut_chord(chord: &str) -> bool {
    if chord.is_empty() || chord.len() > 64 || chord.chars().any(char::is_whitespace) {
        return false;
    }

    let mut parts = chord.split('+').collect::<Vec<_>>();
    if parts.len() > SHORTCUT_MODIFIERS.len() + 1 {
        return false;
    }
    let Some(key) = parts.pop() else {
        return false;
    };
    if key.is_empty()
        || !key
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return false;
    }

    let mut seen = Vec::new();
    for modifier in parts {
        if !SHORTCUT_MODIFIERS.contains(&modifier) || seen.contains(&modifier) {
            return false;
        }
        seen.push(modifier);
    }
    true
}

/// Validates the `keys` array: 1..=4 unique, well-formed chords.
#[must_use]
pub fn is_valid_shortcut_keys(keys: &[String]) -> bool {
    if keys.is_empty() || keys.len() > MAX_SHORTCUT_CHORDS {
        return false;
    }
    key_chords_are_unique_and_valid(keys)
}

fn key_chords_are_unique_and_valid(keys: &[String]) -> bool {
    let mut seen = Vec::new();
    for chord in keys {
        if !is_valid_shortcut_chord(chord) || seen.contains(chord) {
            return false;
        }
        seen.push(chord.clone());
    }
    true
}

/// Panel edge values (PACK_SPEC.md §6.1).
pub const PANEL_LOCATIONS: [&str; 4] = ["top", "bottom", "left", "right"];

/// Panel alignment values.
pub const PANEL_ALIGNMENTS: [&str; 3] = ["left", "center", "right"];

/// Panel hiding modes.
pub const PANEL_HIDING: [&str; 3] = ["none", "autohide", "dodgewindows"];

/// Panel widget operations.
pub const PANEL_WIDGET_OPS: [&str; 3] = ["add", "configure", "remove"];

/// Curated widget types from the validated prototype session (CAPABILITY_MATRIX.md).
pub const WIDGET_TYPES: [&str; 7] = [
    "org.kde.plasma.digitalclock",
    "org.kde.plasma.icontasks",
    "org.kde.plasma.kickoff",
    "org.kde.plasma.marginsseparator",
    "org.kde.plasma.pager",
    "org.kde.plasma.showdesktop",
    "org.kde.plasma.systemtray",
];

/// Whether a widget type is on the curated allowlist.
#[must_use]
pub fn is_widget_type_allowed(widget_type: &str) -> bool {
    WIDGET_TYPES.contains(&widget_type)
}

/// Virtual-desktop operations (PACK_SPEC.md §6.1).
pub const VIRTUAL_DESKTOP_OPS: [&str; 3] = ["add", "rename", "remove"];

/// Look-and-feel components a pack may decompose (PACK_SPEC.md §6.1).
pub const LOOK_AND_FEEL_COMPONENTS: [&str; 6] = [
    "colors",
    "icons",
    "cursors",
    "plasma_style",
    "decorations",
    "wallpaper",
];

/// Whether a look-and-feel component name is in the closed set.
#[must_use]
pub fn is_look_and_feel_component(component: &str) -> bool {
    LOOK_AND_FEEL_COMPONENTS.contains(&component)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        ConfigTarget, ConfigValueType, config_key_capabilities, config_key_type,
        is_engine_owned_config, is_font_key, is_shortcut_allowed, is_valid_shortcut_chord,
        is_valid_shortcut_keys, is_widget_type_allowed,
    };
    use crate::core::capability::Capability;

    #[test]
    fn font_keys_are_allowlisted_with_their_owners() {
        for key in super::FONT_KEYS {
            assert!(is_font_key(key));
            assert_eq!(
                config_key_type(ConfigTarget::Kdeglobals, "General", key),
                Some(ConfigValueType::String)
            );
            assert_eq!(
                config_key_capabilities(ConfigTarget::Kdeglobals, "General", key),
                &[Capability::Fonts, Capability::KdeGlobal]
            );
        }
    }

    #[test]
    fn appearance_managed_keys_are_refused_through_settings() {
        for (group, key) in [
            ("General", "ColorScheme"),
            ("General", "cursorTheme"),
            ("General", "cursorSize"),
            ("Icons", "Theme"),
            ("org.kde.kdecoration2", "theme"),
            ("org.kde.kdecoration2", "library"),
        ] {
            assert!(
                is_engine_owned_config(ConfigTarget::Kdeglobals, group, key),
                "{group}/{key}"
            );
            assert_eq!(
                config_key_type(ConfigTarget::Kdeglobals, group, key),
                None,
                "{group}/{key}"
            );
        }
    }

    #[test]
    fn effect_toggles_are_boolean_and_effect_keys_are_not_yet_configurable() {
        for effect in super::KWIN_EFFECTS {
            assert_eq!(
                config_key_type(ConfigTarget::Kwinrc, "Plugins", &format!("{effect}Enabled")),
                Some(ConfigValueType::Boolean)
            );
            assert_eq!(
                config_key_type(
                    ConfigTarget::Kwinrc,
                    &format!("Effect-{effect}"),
                    "anything"
                ),
                None
            );
        }
        assert_eq!(
            config_key_type(ConfigTarget::Kwinrc, "Plugins", "unknownEnabled"),
            None
        );
        assert_eq!(
            config_key_type(ConfigTarget::Kwinrc, "Plugins", "blur"),
            None
        );
    }

    #[test]
    fn unsupported_targets_and_keys_are_refused() {
        assert_eq!(ConfigTarget::parse("plasmarc"), None);
        assert_eq!(ConfigTarget::parse("kglobalshortcutsrc"), None);
        assert_eq!(
            ConfigTarget::parse("kdeglobals"),
            Some(ConfigTarget::Kdeglobals)
        );
        assert_eq!(
            config_key_type(ConfigTarget::Kdeglobals, "KDE", "SingleClick"),
            None
        );
        assert_eq!(
            config_key_type(ConfigTarget::Kdeglobals, "Icons", "Theme"),
            None
        );
    }

    #[test]
    fn shortcut_grammar_accepts_documented_chords() {
        for chord in [
            "Meta+D",
            "Ctrl+Alt+T",
            "Shift+Print",
            "F11",
            "Meta+Ctrl+Alt+Shift+K",
        ] {
            assert!(is_valid_shortcut_chord(chord), "{chord}");
        }
        for chord in [
            "",
            "+",
            "Meta+",
            "+D",
            "Super+D",
            "Meta+Meta+D",
            "Ctrl + D",
            "Meta+D+E+F+G",
        ] {
            assert!(!is_valid_shortcut_chord(chord), "{chord}");
        }
    }

    #[test]
    fn shortcut_keys_are_unique_and_bounded() {
        let keys = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
        assert!(is_valid_shortcut_keys(&keys(&["Meta+D"])));
        assert!(is_valid_shortcut_keys(&keys(&["Meta+D", "Meta+E"])));
        assert!(!is_valid_shortcut_keys(&keys(&[])));
        assert!(!is_valid_shortcut_keys(&keys(&["Meta+D", "Meta+D"])));
        assert!(!is_valid_shortcut_keys(&keys(&[
            "Meta+1", "Meta+2", "Meta+3", "Meta+4", "Meta+5"
        ])));
    }

    #[test]
    fn shortcut_components_and_actions_are_closed() {
        assert!(is_shortcut_allowed("kwin", "Show Desktop"));
        assert!(is_shortcut_allowed("plasmashell", "Show Dashboard"));
        assert!(!is_shortcut_allowed("kwin", "Do Something Else"));
        assert!(!is_shortcut_allowed("gnome", "Show Desktop"));
    }

    #[test]
    fn widget_types_are_closed() {
        assert!(is_widget_type_allowed("org.kde.plasma.digitalclock"));
        assert!(!is_widget_type_allowed("com.example.evil"));
    }
}
