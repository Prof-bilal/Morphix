//! Manifest expansion: appearance sugar to concrete operations (P3-04, part 1).
//!
//! Pure constructors: manifest declarations become `(capability, target, action)`
//! triples. Pre-state capture, collision checks, and descriptions belong to
//! [`crate::planning::planner`] and [`crate::planning::diff`].

use crate::core::allowlist::ConfigTarget;
use crate::core::capability::Capability;
use crate::core::ids::{AssetKey, PackId};
use crate::core::resource::AssetKind;
use crate::desktop::kde::targets::{
    ConfigValue, ResolvedTarget, installed_asset_path, resolve_asset_target,
    resolve_wallpaper_target,
};
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::packs::manifest::{Manifest, SettingValue};

use super::plan::{OperationAction, VerificationLevel};

/// One expanded operation before pre-state capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedOp {
    /// The capability that must grant this operation.
    pub capability: Capability,
    /// Engine-resolved target.
    pub target: ResolvedTarget,
    /// What the engine will do.
    pub action: OperationAction,
    /// How apply will verify it.
    pub verification: VerificationLevel,
}

/// Expands every `[appearance]` field into install plus select operations (P3-04).
pub fn expand_appearance(manifest: &Manifest) -> Result<Vec<ExpandedOp>> {
    let Some(appearance) = &manifest.appearance else {
        return Ok(Vec::new());
    };
    if appearance.is_empty() {
        return Ok(Vec::new());
    }
    let pack_id = &manifest.pack.id;
    let mut operations = Vec::new();
    if let Some(key) = &appearance.color_scheme {
        push_asset_select(
            &mut operations,
            manifest,
            pack_id,
            AssetKind::ColorScheme,
            key,
            ConfigTarget::Kdeglobals,
            "General",
            "ColorScheme",
        )?;
    }
    if let Some(key) = &appearance.icon_theme {
        push_asset_select(
            &mut operations,
            manifest,
            pack_id,
            AssetKind::IconTheme,
            key,
            ConfigTarget::Kdeglobals,
            "Icons",
            "Theme",
        )?;
    }
    if let Some(key) = &appearance.cursor_theme {
        push_asset_select(
            &mut operations,
            manifest,
            pack_id,
            AssetKind::CursorTheme,
            key,
            ConfigTarget::Kdeglobals,
            "General",
            "cursorTheme",
        )?;
    }
    if let Some(size) = appearance.cursor_size {
        operations.push(ExpandedOp {
            capability: Capability::Cursors,
            target: ResolvedTarget::named_config("kcminputrc", "Mouse", "cursorSize"),
            action: OperationAction::WriteConfigValue {
                file: "kcminputrc".to_owned(),
                group: "Mouse".to_owned(),
                key: "cursorSize".to_owned(),
                value: ConfigValue::Integer(size),
            },
            verification: VerificationLevel::StateVerified,
        });
    }
    if let Some(key) = &appearance.plasma_style {
        push_plasma_style(&mut operations, manifest, pack_id, key)?;
    }
    if let Some(key) = &appearance.window_decoration {
        push_window_decoration(&mut operations, manifest, pack_id, key)?;
    }
    if let Some(key) = &appearance.wallpaper {
        push_wallpaper(&mut operations, manifest, pack_id, key)?;
    }
    if let Some(look_and_feel) = &appearance.look_and_feel {
        super::expand_more::push_look_and_feel(&mut operations, manifest, look_and_feel)?;
    }
    Ok(operations)
}

/// Validates that `key` names a declared asset of `kind`.
fn declared_asset_path(manifest: &Manifest, kind: AssetKind, key: &str) -> Result<String> {
    let parsed = AssetKey::parse(key).map_err(|_| {
        MorphixError::new(
            ErrorCode::AssetMissing,
            "appearance references an invalid asset key",
        )
        .with_context("asset_kind", kind.as_str())
        .with_context("asset_key", key)
    })?;
    manifest
        .assets
        .for_kind(kind)
        .get(&parsed)
        .cloned()
        .ok_or_else(|| {
            MorphixError::new(
                ErrorCode::AssetMissing,
                "appearance references an asset that is not declared",
            )
            .with_context("asset_kind", kind.as_str())
            .with_context("asset_key", key)
        })
}

/// Install plus config-select for one keyed asset.
#[allow(clippy::too_many_arguments)]
fn push_asset_select(
    operations: &mut Vec<ExpandedOp>,
    manifest: &Manifest,
    pack_id: &PackId,
    kind: AssetKind,
    key: &str,
    file: ConfigTarget,
    group: &str,
    config_key: &str,
) -> Result<()> {
    let source = declared_asset_path(manifest, kind, key)?;
    let parsed = AssetKey::parse(key).map_err(|_| {
        MorphixError::new(
            ErrorCode::AssetMissing,
            "appearance references an invalid asset key",
        )
    })?;
    let relative = installed_asset_path(pack_id, kind, &parsed, Some(&source))?;
    let installed = crate::core::ids::InstalledName::derive(pack_id, &parsed)?;
    let target = resolve_asset_target(pack_id, kind, &parsed, relative.clone());
    let tree_hash = format!("plan:{installed}");
    operations.push(ExpandedOp {
        capability: capability_for_kind(kind),
        target,
        action: OperationAction::InstallAsset {
            kind,
            asset_key: key.to_owned(),
            installed_name: installed.as_str().to_owned(),
            target: relative,
            tree_hash,
            is_directory: kind.is_directory(),
        },
        verification: VerificationLevel::StateVerified,
    });
    let select_target = ResolvedTarget::config_key(file, group, config_key);
    operations.push(ExpandedOp {
        capability: capability_for_kind(kind),
        target: select_target,
        action: OperationAction::WriteConfigValue {
            file: file.as_str().to_owned(),
            group: group.to_owned(),
            key: config_key.to_owned(),
            value: ConfigValue::String(installed.as_str().to_owned()),
        },
        verification: verification_for_select(kind),
    });
    Ok(())
}

/// Plasma style selection writes `plasmarc [Theme] name` (ARCHITECTURE.md 9.2).
fn push_plasma_style(
    operations: &mut Vec<ExpandedOp>,
    manifest: &Manifest,
    pack_id: &PackId,
    key: &str,
) -> Result<()> {
    let source = declared_asset_path(manifest, AssetKind::PlasmaStyle, key)?;
    let parsed = AssetKey::parse(key).map_err(|_| {
        MorphixError::new(
            ErrorCode::AssetMissing,
            "appearance references an invalid asset key",
        )
    })?;
    let relative = installed_asset_path(pack_id, AssetKind::PlasmaStyle, &parsed, Some(&source))?;
    let installed = crate::core::ids::InstalledName::derive(pack_id, &parsed)?;
    let target = resolve_asset_target(pack_id, AssetKind::PlasmaStyle, &parsed, relative.clone());
    operations.push(ExpandedOp {
        capability: Capability::PlasmaStyle,
        target,
        action: OperationAction::InstallAsset {
            kind: AssetKind::PlasmaStyle,
            asset_key: key.to_owned(),
            installed_name: installed.as_str().to_owned(),
            target: relative,
            tree_hash: format!("plan:{installed}"),
            is_directory: true,
        },
        verification: VerificationLevel::StateVerified,
    });
    operations.push(ExpandedOp {
        capability: Capability::PlasmaStyle,
        target: ResolvedTarget::named_config("plasmarc", "Theme", "name"),
        action: OperationAction::WriteConfigValue {
            file: "plasmarc".to_owned(),
            group: "Theme".to_owned(),
            key: "name".to_owned(),
            value: ConfigValue::String(installed.as_str().to_owned()),
        },
        verification: VerificationLevel::StateVerified,
    });
    Ok(())
}

/// Aurorae decoration: install plus `kwinrc [org.kde.kdecoration2]` writes.
fn push_window_decoration(
    operations: &mut Vec<ExpandedOp>,
    manifest: &Manifest,
    pack_id: &PackId,
    key: &str,
) -> Result<()> {
    let source = declared_asset_path(manifest, AssetKind::WindowDecoration, key)?;
    let parsed = AssetKey::parse(key).map_err(|_| {
        MorphixError::new(
            ErrorCode::AssetMissing,
            "appearance references an invalid asset key",
        )
    })?;
    let relative =
        installed_asset_path(pack_id, AssetKind::WindowDecoration, &parsed, Some(&source))?;
    let installed = crate::core::ids::InstalledName::derive(pack_id, &parsed)?;
    let target = resolve_asset_target(
        pack_id,
        AssetKind::WindowDecoration,
        &parsed,
        relative.clone(),
    );
    operations.push(ExpandedOp {
        capability: Capability::WindowDecorations,
        target,
        action: OperationAction::InstallAsset {
            kind: AssetKind::WindowDecoration,
            asset_key: key.to_owned(),
            installed_name: installed.as_str().to_owned(),
            target: relative,
            tree_hash: format!("plan:{installed}"),
            is_directory: true,
        },
        verification: VerificationLevel::StateVerified,
    });
    for (config_key, value) in [
        ("library", "org.kde.kwin.aurorae"),
        ("theme", installed.as_str()),
    ] {
        operations.push(ExpandedOp {
            capability: Capability::WindowDecorations,
            target: ResolvedTarget::config_key(
                ConfigTarget::Kwinrc,
                "org.kde.kdecoration2",
                config_key,
            ),
            action: OperationAction::WriteConfigValue {
                file: ConfigTarget::Kwinrc.as_str().to_owned(),
                group: "org.kde.kdecoration2".to_owned(),
                key: config_key.to_owned(),
                value: ConfigValue::String(value.to_owned()),
            },
            verification: VerificationLevel::StateVerified,
        });
    }
    Ok(())
}

/// Wallpaper: install the image plus one per-desktop `SetWallpaper`.
fn push_wallpaper(
    operations: &mut Vec<ExpandedOp>,
    manifest: &Manifest,
    pack_id: &PackId,
    key: &str,
) -> Result<()> {
    let source = declared_asset_path(manifest, AssetKind::Wallpaper, key)?;
    let parsed = AssetKey::parse(key).map_err(|_| {
        MorphixError::new(
            ErrorCode::AssetMissing,
            "appearance references an invalid asset key",
        )
    })?;
    let relative = installed_asset_path(pack_id, AssetKind::Wallpaper, &parsed, Some(&source))?;
    let installed = crate::core::ids::InstalledName::derive(pack_id, &parsed)?;
    let target = resolve_asset_target(pack_id, AssetKind::Wallpaper, &parsed, relative.clone());
    operations.push(ExpandedOp {
        capability: Capability::Wallpaper,
        target,
        action: OperationAction::InstallAsset {
            kind: AssetKind::Wallpaper,
            asset_key: key.to_owned(),
            installed_name: installed.as_str().to_owned(),
            target: relative.clone(),
            tree_hash: format!("plan:{installed}"),
            is_directory: false,
        },
        verification: VerificationLevel::StateVerified,
    });
    operations.push(ExpandedOp {
        capability: Capability::Wallpaper,
        target: resolve_wallpaper_target(pack_id, key),
        action: OperationAction::SetWallpaper {
            pack_id: pack_id.as_str().to_owned(),
            image_path: relative,
            desktop: "all".to_owned(),
        },
        verification: VerificationLevel::LiveVerified,
    });
    Ok(())
}

/// The capability granting one asset kind's install plus select pair.
fn capability_for_kind(kind: AssetKind) -> Capability {
    match kind {
        AssetKind::ColorScheme => Capability::ColorScheme,
        AssetKind::IconTheme => Capability::Icons,
        AssetKind::CursorTheme => Capability::Cursors,
        AssetKind::PlasmaStyle => Capability::PlasmaStyle,
        AssetKind::WindowDecoration => Capability::WindowDecorations,
        AssetKind::Wallpaper => Capability::Wallpaper,
    }
}

/// Icons apply to config but become live only after a shell restart (ARCH 9.2).
fn verification_for_select(kind: AssetKind) -> VerificationLevel {
    match kind {
        AssetKind::IconTheme => VerificationLevel::RestartVerified,
        _ => VerificationLevel::StateVerified,
    }
}

/// Converts a manifest `[[settings]]` value into the shared config value type.
#[must_use]
pub fn setting_value_to_config(value: &SettingValue) -> ConfigValue {
    match value {
        SettingValue::Boolean(inner) => ConfigValue::Boolean(*inner),
        SettingValue::Integer(inner) => ConfigValue::Integer(*inner),
        SettingValue::String(inner) => ConfigValue::String(inner.clone()),
        SettingValue::Strings(inner) => ConfigValue::Strings(inner.clone()),
    }
}

/// Expands `[[settings]]` rows into config writes (P3-05).
///
/// Appearance-managed keys are refused here: they are reachable only through
/// `[appearance]` with the matching capability (PACK_SPEC.md 5.7).
pub fn expand_settings(manifest: &Manifest) -> Result<Vec<ExpandedOp>> {
    let mut operations = Vec::new();
    for entry in &manifest.settings {
        let target = ConfigTarget::parse(&entry.target).ok_or_else(|| {
            MorphixError::new(
                ErrorCode::ConfigTargetUnsupported,
                "settings target is not in the fixed map",
            )
            .with_context("target", &entry.target)
        })?;
        if crate::core::allowlist::is_engine_owned_config(target, &entry.group, &entry.key) {
            return Err(MorphixError::new(
                ErrorCode::ConfigKeyNotAllowed,
                "appearance-managed keys are reachable only through [appearance]",
            )
            .with_context("group", &entry.group)
            .with_context("key", &entry.key));
        }
        let resolved = ResolvedTarget::config_key(target, &entry.group, &entry.key);
        operations.push(ExpandedOp {
            capability: crate::core::allowlist::config_key_capabilities(
                target,
                &entry.group,
                &entry.key,
            )
            .first()
            .copied()
            .unwrap_or(Capability::KdeGlobal),
            target: resolved,
            action: OperationAction::WriteConfigValue {
                file: target.as_str().to_owned(),
                group: entry.group.clone(),
                key: entry.key.clone(),
                value: setting_value_to_config(&entry.value),
            },

            verification: VerificationLevel::StateVerified,
        });
    }
    Ok(operations)
}

/// Expands `[[kwin.effects]]` rows into effect toggles (P3-05).
pub fn expand_effects(manifest: &Manifest) -> Result<Vec<ExpandedOp>> {
    let Some(kwin) = &manifest.kwin else {
        return Ok(Vec::new());
    };
    let mut operations = Vec::new();
    for effect in &kwin.effects {
        if !crate::core::allowlist::is_kwin_effect(&effect.id) {
            return Err(MorphixError::new(
                ErrorCode::EffectNotAllowed,
                "KWin effect is not on the allowlist",
            )
            .with_context("effect", &effect.id));
        }
        operations.push(ExpandedOp {
            capability: Capability::KwinEffects,
            target: crate::desktop::kde::targets::resolve_effect_target(&effect.id),
            action: OperationAction::SetKwinEffect {
                id: effect.id.clone(),
                enabled: effect.enabled,
            },
            verification: VerificationLevel::LiveVerified,
        });
    }
    Ok(operations)
}

/// Expands `[[shortcuts]]` rows into shortcut writes (P3-05).
pub fn expand_shortcuts(manifest: &Manifest) -> Result<Vec<ExpandedOp>> {
    let mut operations = Vec::new();
    for entry in &manifest.shortcuts {
        if !crate::core::allowlist::is_shortcut_allowed(&entry.component, &entry.action) {
            return Err(MorphixError::new(
                ErrorCode::ShortcutNotAllowed,
                "shortcut component or action is not allowlisted",
            )
            .with_context("component", &entry.component)
            .with_context("action", &entry.action));
        }
        operations.push(ExpandedOp {
            capability: Capability::Shortcuts,
            target: crate::desktop::kde::targets::resolve_shortcut_target(
                &entry.component,
                &entry.action,
            ),
            action: OperationAction::WriteShortcut {
                component: entry.component.clone(),
                action: entry.action.clone(),
                keys: entry.keys.clone(),
            },
            verification: VerificationLevel::LiveVerified,
        });
    }
    Ok(operations)
}

//__EXPAND_MORE__
