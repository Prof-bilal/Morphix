//! Panel, desktop-behaviour, and global-theme expansion (P3-04/P3-05, part 2).
//!
//! Pure constructors over manifest declarations. Live inventories (panels, desktops)
//! arrive as explicit parameters so expansion stays a pure function of its inputs.

use crate::core::allowlist::{PANEL_ALIGNMENTS, PANEL_HIDING, PANEL_LOCATIONS};
use crate::core::capability::Capability;
use crate::desktop::kde::targets::{
    ConfigValue, resolve_panel_property_target, resolve_panel_target,
    resolve_virtual_desktop_target, resolve_widget_target,
};
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::packs::manifest::{LookAndFeel, Manifest};

use super::expand::ExpandedOp;
use super::plan::{OperationAction, VerificationLevel};

/// First existing panel targeted when a pack declares panel properties.
///
/// Panel IDs are runtime-assigned by Plasma; the planner targets the first live panel
/// and records the choice in the operation description.
pub const DEFAULT_PANEL_ID: u32 = 0;

/// Expands `[panel]` declarations into panel operations.
pub fn expand_panel(manifest: &Manifest, live_panels: &[u32]) -> Result<Vec<ExpandedOp>> {
    let Some(panel) = &manifest.panel else {
        return Ok(Vec::new());
    };
    let mut operations = Vec::new();
    let target_panel = live_panels.first().copied().unwrap_or(DEFAULT_PANEL_ID);
    if panel.create {
        operations.push(ExpandedOp {
            capability: Capability::PanelCreate,
            target: resolve_panel_target(target_panel),
            action: OperationAction::CreatePanel {
                panel_type: "org.kde.panel".to_owned(),
            },
            verification: VerificationLevel::LiveVerified,
        });
    }
    for properties in &panel.properties {
        if properties.is_empty() {
            return Err(MorphixError::new(
                ErrorCode::ValueInvalid,
                "a panel properties row changes nothing",
            ));
        }
        if let Some(location) = &properties.location {
            if !PANEL_LOCATIONS.contains(&location.as_str()) {
                return Err(invalid_panel_value("location", location));
            }
            operations.push(panel_property_op(
                target_panel,
                "location",
                location.clone(),
            ));
        }
        if let Some(height) = properties.height {
            if !(16..=128).contains(&height) {
                return Err(invalid_panel_value("height", &height.to_string()));
            }
            operations.push(panel_property_op(
                target_panel,
                "height",
                height.to_string(),
            ));
        }
        if let Some(alignment) = &properties.alignment {
            if !PANEL_ALIGNMENTS.contains(&alignment.as_str()) {
                return Err(invalid_panel_value("alignment", alignment));
            }
            operations.push(panel_property_op(
                target_panel,
                "alignment",
                alignment.clone(),
            ));
        }
        if let Some(floating) = properties.floating {
            operations.push(panel_property_op(
                target_panel,
                "floating",
                floating.to_string(),
            ));
        }
        if let Some(hiding) = &properties.hiding {
            if !PANEL_HIDING.contains(&hiding.as_str()) {
                return Err(invalid_panel_value("hiding", hiding));
            }
            operations.push(panel_property_op(target_panel, "hiding", hiding.clone()));
        }
    }
    for widget in &panel.widgets {
        operations.push(expand_widget(manifest, widget, live_panels)?);
    }
    Ok(operations)
}

/// One panel-property operation.
fn panel_property_op(panel_id: u32, property: &str, value: String) -> ExpandedOp {
    ExpandedOp {
        capability: Capability::PanelProperties,
        target: resolve_panel_property_target(panel_id, property),
        action: OperationAction::SetPanelProperty {
            panel_id,
            property: property.to_owned(),
            value,
        },
        verification: VerificationLevel::LiveVerified,
    }
}

/// Expands one `[[panel.widgets]]` row.
fn expand_widget(
    manifest: &Manifest,
    widget: &crate::packs::manifest::PanelWidget,
    live_panels: &[u32],
) -> Result<ExpandedOp> {
    if !crate::core::allowlist::is_widget_type_allowed(&widget.widget_type) {
        return Err(MorphixError::new(
            ErrorCode::UnsupportedResource,
            "widget type is not on the curated allowlist",
        )
        .with_context("widget_type", &widget.widget_type));
    }
    if !widget.safe {
        return Err(MorphixError::new(
            ErrorCode::UnsupportedResource,
            "a widget declaration must carry the explicit safety flag",
        )
        .with_context("widget_type", &widget.widget_type));
    }
    let panel_index = usize::try_from(widget.panel).map_err(|_| {
        MorphixError::new(ErrorCode::ValueInvalid, "widget panel index is negative")
            .with_context("panel", widget.panel.to_string())
    })?;
    let panel_id = live_panels
        .get(panel_index)
        .copied()
        .unwrap_or(DEFAULT_PANEL_ID);
    match widget.op.as_str() {
        "add" => Ok(ExpandedOp {
            capability: Capability::PanelWidgets,
            target: resolve_widget_target(panel_id, &widget.widget_type, -1),
            action: OperationAction::AddWidget {
                panel_id,
                widget_type: widget.widget_type.clone(),
                index: -1,
            },
            verification: VerificationLevel::LiveVerified,
        }),
        "remove" => Err(MorphixError::new(
            ErrorCode::UnsupportedResource,
            "widget removal needs a live widget inventory; supply panel state before planning",
        )
        .with_context("widget_type", &widget.widget_type)
        .with_context("pack_id", manifest.pack.id.as_str())),
        "configure" => Err(MorphixError::new(
            ErrorCode::UnsupportedResource,
            "widget configuration rows carry no key or value in schema v1",
        )
        .with_context("widget_type", &widget.widget_type)),
        other => Err(
            MorphixError::new(ErrorCode::ValueInvalid, "unknown widget operation")
                .with_context("op", other),
        ),
    }
}

/// Expands `[desktop_behavior]` virtual-desktop rows.
pub fn expand_desktop_behavior(
    manifest: &Manifest,
    live_desktops: &[(Option<String>, String)],
) -> Result<Vec<ExpandedOp>> {
    let Some(behavior) = &manifest.desktop_behavior else {
        return Ok(Vec::new());
    };
    let mut operations = Vec::new();
    for desktop in &behavior.virtual_desktops {
        match desktop.op.as_str() {
            "add" => {
                let name = desktop.name.clone().ok_or_else(|| {
                    MorphixError::new(
                        ErrorCode::ValueInvalid,
                        "adding a virtual desktop needs a name",
                    )
                })?;
                operations.push(ExpandedOp {
                    capability: Capability::DesktopBehavior,
                    target: resolve_virtual_desktop_target("add", Some(&name)),
                    action: OperationAction::ManageVirtualDesktop {
                        action: "add".to_owned(),
                        name: Some(name),
                    },
                    verification: VerificationLevel::LiveVerified,
                });
            }
            "rename" => {
                let name = desktop.name.clone().ok_or_else(|| {
                    MorphixError::new(
                        ErrorCode::ValueInvalid,
                        "renaming a virtual desktop needs a name",
                    )
                })?;
                if !live_desktops.iter().any(|(_, existing)| existing == &name)
                    && !live_desktops.is_empty()
                {
                    return Err(MorphixError::new(
                        ErrorCode::UnsupportedResource,
                        "rename target does not match a live desktop; remove and add instead",
                    )
                    .with_context("name", &name));
                }
                operations.push(ExpandedOp {
                    capability: Capability::DesktopBehavior,
                    target: resolve_virtual_desktop_target("rename", Some(&name)),
                    action: OperationAction::ManageVirtualDesktop {
                        action: "rename".to_owned(),
                        name: Some(name),
                    },
                    verification: VerificationLevel::LiveVerified,
                });
            }
            "remove" => {
                operations.push(ExpandedOp {
                    capability: Capability::DesktopBehavior,
                    target: resolve_virtual_desktop_target("remove", desktop.name.as_deref()),
                    action: OperationAction::ManageVirtualDesktop {
                        action: "remove".to_owned(),
                        name: desktop.name.clone(),
                    },
                    verification: VerificationLevel::LiveVerified,
                });
            }
            other => {
                return Err(MorphixError::new(
                    ErrorCode::ValueInvalid,
                    "unknown virtual desktop operation",
                )
                .with_context("op", other));
            }
        }
    }
    Ok(operations)
}

/// Expands `[appearance.look_and_feel]` into a marker plus per-component primitives.
///
/// The full package-defaults decomposition needs the installed package reader that
/// arrives with the Phase 6 adapter. At planner level the expansion carries the marker
/// write plus one primitive per component so every component stays individually
/// reversible (TRANSACTIONS.md 5); Phase 6 refines the per-component values against
/// the package's `contents/defaults`.
pub fn push_look_and_feel(
    operations: &mut Vec<ExpandedOp>,
    manifest: &Manifest,
    look_and_feel: &LookAndFeel,
) -> Result<()> {
    for component in &look_and_feel.components {
        if !crate::core::allowlist::is_look_and_feel_component(component) {
            return Err(MorphixError::new(
                ErrorCode::ValueInvalid,
                "unknown look-and-feel component",
            )
            .with_context("component", component));
        }
    }
    if look_and_feel.package.trim().is_empty() {
        return Err(MorphixError::new(
            ErrorCode::ValueInvalid,
            "look-and-feel package id is empty",
        ));
    }
    let mut expanded = Vec::new();
    expanded.push(OperationAction::WriteConfigValue {
        file: "kdeglobals".to_owned(),
        group: "KDE".to_owned(),
        key: "LookAndFeelPackage".to_owned(),
        value: ConfigValue::String(look_and_feel.package.clone()),
    });
    for component in &look_and_feel.components {
        expanded.push(component_placeholder(component, look_and_feel));
    }
    let target = crate::desktop::kde::targets::resolve_global_theme_target(&look_and_feel.package);
    let action = OperationAction::ApplyGlobalTheme {
        package_id: look_and_feel.package.clone(),
        components: look_and_feel.components.clone(),
        expanded,
    };
    operations.push(ExpandedOp {
        capability: Capability::LookAndFeel,
        target,
        action,
        verification: VerificationLevel::StateVerified,
    });
    let _ = manifest;
    Ok(())
}

/// One reversible placeholder primitive per component, refined in Phase 6.
fn component_placeholder(component: &str, look_and_feel: &LookAndFeel) -> OperationAction {
    let package = look_and_feel.package.clone();
    match component {
        "colors" => OperationAction::WriteConfigValue {
            file: "kdeglobals".to_owned(),
            group: "General".to_owned(),
            key: "ColorScheme".to_owned(),
            value: ConfigValue::String(package),
        },
        "icons" => OperationAction::WriteConfigValue {
            file: "kdeglobals".to_owned(),
            group: "Icons".to_owned(),
            key: "Theme".to_owned(),
            value: ConfigValue::String(package),
        },
        "cursors" => OperationAction::WriteConfigValue {
            file: "kdeglobals".to_owned(),
            group: "General".to_owned(),
            key: "cursorTheme".to_owned(),
            value: ConfigValue::String(package),
        },
        "plasma_style" => OperationAction::WriteConfigValue {
            file: "plasmarc".to_owned(),
            group: "Theme".to_owned(),
            key: "name".to_owned(),
            value: ConfigValue::String(package),
        },
        "decorations" => OperationAction::WriteConfigValue {
            file: "kwinrc".to_owned(),
            group: "org.kde.kdecoration2".to_owned(),
            key: "theme".to_owned(),
            value: ConfigValue::String(package),
        },
        _ => OperationAction::SetWallpaper {
            pack_id: package,
            image_path: String::new(),
            desktop: "all".to_owned(),
        },
    }
}

/// Refusal for an out-of-range panel value.
fn invalid_panel_value(property: &str, value: &str) -> MorphixError {
    MorphixError::new(
        ErrorCode::ValueInvalid,
        "panel property value is out of range",
    )
    .with_context("property", property)
    .with_context("value", value)
}
