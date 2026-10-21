//! Human-readable diffs: old value compared with new value (P3-07).
//!
//! Generated in Rust so the UI renders text it never computes. Every description names
//! the resource, the previous state, and the planned state.

use super::plan::OperationAction;
use crate::desktop::kde::targets::ResourceState;

/// Builds the user-facing description for one operation.
#[must_use]
pub fn describe(
    action: &OperationAction,
    previous: &Option<ResourceState>,
    expected: &ResourceState,
) -> String {
    let before = previous
        .as_ref()
        .map(ResourceState::render)
        .unwrap_or_else(|| "(absent)".to_owned());
    let after = expected.render();
    match action {
        OperationAction::InstallAsset { installed_name, .. } => {
            format!("Install asset {installed_name} (was: {before})")
        }
        OperationAction::WriteConfigValue {
            file, group, key, ..
        } => {
            format!("Set {file} [{group}] {key} from {before} to {after}")
        }
        OperationAction::WriteShortcut {
            component, action, ..
        } => format!("Set shortcut {component} / {action} from {before} to {after}"),
        OperationAction::SetKwinEffect { id, enabled, .. } => {
            let state = if *enabled { "enable" } else { "disable" };
            format!("{state} KWin effect {id} (was: {before})")
        }
        OperationAction::SetWallpaper { desktop, .. } => {
            format!("Set wallpaper for {desktop} from {before} to {after}")
        }
        OperationAction::SetPanelProperty {
            panel_id, property, ..
        } => format!("Set panel {panel_id} {property} from {before} to {after}"),
        OperationAction::CreatePanel { panel_type } => {
            format!("Create panel of type {panel_type}")
        }
        OperationAction::RemovePanel { panel_id } => {
            format!("Remove panel {panel_id} (captured: {before})")
        }
        OperationAction::AddWidget {
            panel_id,
            widget_type,
            ..
        } => format!("Add widget {widget_type} to panel {panel_id}"),
        OperationAction::ConfigureWidget {
            panel_id,
            widget_type,
            ..
        } => format!("Configure widget {widget_type} on panel {panel_id}: {before} to {after}"),
        OperationAction::RemoveWidget {
            panel_id,
            widget_type,
            ..
        } => format!("Remove widget {widget_type} from panel {panel_id} (was: {before})"),
        OperationAction::ApplyGlobalTheme {
            package_id,
            components,
            ..
        } => format!(
            "Apply global theme {package_id} ({}: {})",
            components.len(),
            components.join(", ")
        ),
        OperationAction::ManageVirtualDesktop { action, name } => match name {
            Some(name) => format!("Virtual desktop {action} {name} (was: {before})"),
            None => format!("Virtual desktop {action} (was: {before})"),
        },
    }
}

/// One rendered old-to-new line for plan detail views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// Conflict-scan identity of the resource.
    pub identity: String,
    /// Previous rendering.
    pub before: String,
    /// Planned rendering.
    pub after: String,
    /// Full one-line description.
    pub summary: String,
}

/// Renders one operation as a [`DiffLine`].
#[must_use]
pub fn diff_line(
    identity: &str,
    action: &OperationAction,
    previous: &Option<ResourceState>,
    expected: &ResourceState,
) -> DiffLine {
    DiffLine {
        identity: identity.to_owned(),
        before: previous
            .as_ref()
            .map(ResourceState::render)
            .unwrap_or_else(|| "(absent)".to_owned()),
        after: expected.render(),
        summary: describe(action, previous, expected),
    }
}
