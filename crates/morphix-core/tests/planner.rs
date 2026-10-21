//! Planner integration tests over injected pre-state (P3-12, P3-13, AC-09/AC-10).
//!
//! Every test builds a manifest from TOML text and plans through [`MemReads`] or the
//! [`RecordingReads`] wrapper. No filesystem writes, no DBus, no storage: the planner
//! under test only sees the injected seam, which is exactly the purity property AC-10
//! requires.

use morphix_core::desktop::kde::targets::ResourceState;
use morphix_core::errors::ErrorCode;
use morphix_core::packs::manifest::Manifest;
use morphix_core::packs::parser::parse_manifest;
use morphix_core::planning::plan::{OperationAction, VerificationLevel};
use morphix_core::planning::planner::plan_pack;
use morphix_core::planning::reads::{MemReads, RecordingReads};

const HEADER: &str = r#"schema_version = 1

[pack]
id = "dev.morphix.testpack"
name = "Test Pack"
version = "1.0.0"
summary = "A planner fixture pack."
description = "A planner fixture pack used by the Phase 3 integration tests."
authors = ["Morphix Tests"]
license = "MIT"
icon = "previews/icon.png"

[compatibility]
desktop = "kde"
plasma = ">=6.0, <7.0"

[[previews]]
image = "previews/icon.png"
caption = "Preview"
"#;

fn manifest(body: &str) -> Manifest {
    let text = format!("{HEADER}\n{body}");
    manifest_raw(&text)
}

fn manifest_raw(text: &str) -> Manifest {
    parse_manifest(text.as_bytes())
        .expect("fixture manifest parses")
        .manifest
}

fn reads_for(manifest: Manifest) -> MemReads {
    let mut reads = MemReads {
        manifest: Some(manifest),
        content_hash: "content-hash-fixture".to_owned(),
        plasma_version: Some("6.3.5".to_owned()),
        session_type: "wayland".to_owned(),
        wallpaper_supported: true,
        ..Default::default()
    };
    reads.all_capabilities_available();
    reads
}

fn color_scheme_body() -> String {
    r#"[capabilities]
requires = ["appearance.color_scheme"]

[assets.color_schemes]
dark = "themes/color-schemes/Dark.colors"

[appearance]
color_scheme = "dark"
"#
    .to_owned()
}

#[test]
fn planning_twice_on_unchanged_state_is_identical() {
    let plan_first = plan_pack(&reads_for(manifest(&color_scheme_body())), 0).expect("first plan");
    let plan_second =
        plan_pack(&reads_for(manifest(&color_scheme_body())), 0).expect("second plan");
    assert_ne!(plan_first.id, plan_second.id, "ids stay unique");
    assert_eq!(plan_first.canonical_json(), plan_second.canonical_json());
    assert_eq!(plan_first.hash, plan_second.hash);
    assert!(plan_first.hash_is_current());
    assert_eq!(plan_first.created_at, "1970-01-01T00:00:00Z");
    assert_eq!(
        plan_first.expires_at, "1970-01-01T00:10:00Z",
        "default TTL is ten minutes past the fixed clock"
    );
}

#[test]
fn changed_prestate_changes_the_hash() {
    let before =
        plan_pack(&reads_for(manifest(&color_scheme_body())), 1_790_000_000).expect("plan");
    let mut changed = reads_for(manifest(&color_scheme_body()));
    changed.config_values.insert(
        (
            "kdeglobals".to_owned(),
            "General".to_owned(),
            "ColorScheme".to_owned(),
        ),
        ResourceState::ConfigValue(morphix_core::desktop::kde::targets::ConfigValue::String(
            "BreezeDark".to_owned(),
        )),
    );
    let after = plan_pack(&changed, 1_790_000_000).expect("plan");
    assert_ne!(before.canonical_json(), after.canonical_json());
    assert_ne!(before.hash, after.hash);
}

#[test]
fn planning_performs_only_reads_and_writes_nothing() {
    let temp = tempfile::tempdir().expect("temp");
    let sentinel = temp.path().join("sentinel.txt");
    std::fs::write(&sentinel, b"untouched").expect("sentinel");
    let before = morphix_core::fs::tree::snapshot(temp.path())
        .expect("snapshot")
        .content_hash();

    let inner = reads_for(manifest(&color_scheme_body()));
    let recording = RecordingReads::new(&inner);
    let plan = plan_pack(&recording, 1_790_000_000).expect("plan");
    assert!(!plan.operations.is_empty());

    let log = recording.log();
    assert!(!log.is_empty(), "the run observed pre-state");
    assert!(
        log.iter().all(|entry| !entry.starts_with("write")),
        "no write-class call exists to record: {log:?}"
    );
    let after = morphix_core::fs::tree::snapshot(temp.path())
        .expect("snapshot")
        .content_hash();
    assert_eq!(before, after, "planner left the sentinel tree untouched");
}

#[test]
fn color_scheme_expands_to_install_plus_select_with_overwrite_warning() {
    let mut reads = reads_for(manifest(&color_scheme_body()));
    reads.config_values.insert(
        (
            "kdeglobals".to_owned(),
            "General".to_owned(),
            "ColorScheme".to_owned(),
        ),
        ResourceState::ConfigValue(morphix_core::desktop::kde::targets::ConfigValue::String(
            "Breeze".to_owned(),
        )),
    );
    let plan = plan_pack(&reads, 1_790_000_000).expect("plan");
    assert_eq!(plan.operations.len(), 2);
    assert!(matches!(
        plan.operations[0].action,
        OperationAction::InstallAsset { .. }
    ));
    assert!(matches!(
        plan.operations[1].action,
        OperationAction::WriteConfigValue { .. }
    ));
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.target
                == Some("config:kdeglobals:General:ColorScheme".to_owned())),
        "existing values are disclosed, not silently replaced"
    );
}

#[test]
fn icon_selection_is_restart_verified() {
    let body = r#"[capabilities]
requires = ["appearance.icons"]

[assets.icon_themes]
cupertino = "icons/Cupertino"

[appearance]
icon_theme = "cupertino"
"#;
    let plan = plan_pack(&reads_for(manifest(body)), 1_790_000_000).expect("plan");
    assert_eq!(plan.operations.len(), 2);
    assert_eq!(
        plan.operations[1].verification,
        VerificationLevel::RestartVerified
    );
    assert!(plan.restart_required);
    assert!(plan.warnings.iter().any(|warning| matches!(
        warning.kind,
        morphix_core::planning::plan::WarningKind::RestartRequired
    )));
}

#[test]
fn settings_effects_and_shortcuts_expand() {
    let body = r#"[capabilities]
requires = ["settings.kde_global", "settings.kwin_effects", "settings.shortcuts"]

[[settings]]
target = "kdeglobals"
group = "General"
key = "font"
value = "Noto Sans,10,-1,5,50,0,0,0,0,0"

[[kwin.effects]]
id = "blur"
enabled = true

[[shortcuts]]
component = "kwin"
action = "Show Desktop"
keys = ["Meta+D"]
"#;
    let plan = plan_pack(&reads_for(manifest(body)), 1_790_000_000).expect("plan");
    assert_eq!(plan.operations.len(), 3);
    assert_eq!(plan.capabilities.len(), 3);
    assert!(
        plan.operations
            .iter()
            .any(|op| matches!(op.action, OperationAction::WriteShortcut { .. }))
    );
    assert_eq!(plan.totals.operation_count, 3);
    assert_eq!(plan.totals.config_operations, 3);
    assert_eq!(plan.totals.asset_operations, 0);
}

#[test]
fn incompatible_plasma_and_desktop_are_refused() {
    let text = format!("{HEADER}\n{}", color_scheme_body()).replace(">=6.0, <7.0", ">=6.5, <7.0");
    let error =
        plan_pack(&reads_for(manifest_raw(&text)), 1_790_000_000).expect_err("incompatible");
    assert_eq!(error.code, ErrorCode::IncompatiblePack);

    let text = format!("{HEADER}\n{}", color_scheme_body())
        .replace("desktop = \"kde\"", "desktop = \"gnome\"");
    let error =
        plan_pack(&reads_for(manifest_raw(&text)), 1_790_000_000).expect_err("wrong desktop");
    assert_eq!(error.code, ErrorCode::IncompatiblePack);
}

#[test]
fn undeclared_use_is_denied_and_unused_requests_fail() {
    let body = r#"[capabilities]
requires = ["settings.kde_global"]

[[settings]]
target = "kdeglobals"
group = "General"
key = "font"
value = "Noto Sans,10"

[[kwin.effects]]
id = "blur"
enabled = true
"#;
    let error = plan_pack(&reads_for(manifest(body)), 1_790_000_000).expect_err("undeclared");
    assert_eq!(error.code, ErrorCode::CapabilityDenied);

    let body = r#"[capabilities]
requires = ["settings.kde_global", "settings.shortcuts"]

[[settings]]
target = "kdeglobals"
group = "General"
key = "font"
value = "Noto Sans,10"
"#;
    let error = plan_pack(&reads_for(manifest(body)), 1_790_000_000).expect_err("unused");
    assert_eq!(error.code, ErrorCode::CapabilityUnused);
}

#[test]
fn duplicate_targets_within_one_plan_are_refused() {
    let body = r#"[capabilities]
requires = ["panel.properties"]

[[panel.properties]]
height = 44

[[panel.properties]]
height = 48
"#;
    let error = plan_pack(&reads_for(manifest(body)), 1_790_000_000).expect_err("duplicate");
    assert_eq!(error.code, ErrorCode::DuplicateTarget);
}

#[test]
fn foreign_targets_are_refused_but_owned_targets_plan() {
    use morphix_core::planning::reads::TargetOwnership;

    let mut foreign = reads_for(manifest(&color_scheme_body()));
    foreign.ownership.insert(
        "color-schemes/testpack-dark.colors".to_owned(),
        TargetOwnership::UnownedPresent {
            hash: Some("h".to_owned()),
        },
    );
    let error = plan_pack(&foreign, 1_790_000_000).expect_err("unowned");
    assert_eq!(error.code, ErrorCode::TargetExists);

    let mut owned = reads_for(manifest(&color_scheme_body()));
    owned.ownership.insert(
        "color-schemes/testpack-dark.colors".to_owned(),
        TargetOwnership::OwnedBy {
            pack_id: "dev.morphix.testpack".to_owned(),
            version: "1.0.0".to_owned(),
            hash: "h".to_owned(),
        },
    );
    assert!(plan_pack(&owned, 1_790_000_000).is_ok());
}

#[test]
fn wallpaper_without_a_channel_installs_and_defers() {
    let body = r#"[capabilities]
requires = ["appearance.wallpaper"]

[assets.wallpapers]
dusk = "wallpapers/dusk.webp"

[appearance]
wallpaper = "dusk"
"#;
    let mut gated = reads_for(manifest(body));
    gated.wallpaper_supported = false;
    let plan = plan_pack(&gated, 1_790_000_000).expect("plan");
    assert_eq!(plan.operations.len(), 1, "only the install remains");
    assert!(matches!(
        plan.operations[0].action,
        OperationAction::InstallAsset { .. }
    ));
    assert!(plan.warnings.iter().any(|warning| matches!(
        warning.kind,
        morphix_core::planning::plan::WarningKind::ManualStep
    )));

    let open = reads_for(manifest(body));
    let plan = plan_pack(&open, 1_790_000_000).expect("plan");
    assert_eq!(plan.operations.len(), 2);
    assert!(matches!(
        plan.operations[1].action,
        OperationAction::SetWallpaper { .. }
    ));
    assert_eq!(
        plan.operations[1].verification,
        VerificationLevel::LiveVerified
    );
}

#[test]
fn panel_create_and_widget_add_expand() {
    let body = r#"[capabilities]
requires = ["panel.properties", "panel.widgets", "panel.create"]

[panel]
create = true

[[panel.properties]]
height = 44
location = "bottom"

[[panel.widgets]]
panel = 0
op = "add"
type = "org.kde.plasma.digitalclock"
safe = true
"#;
    let plan = plan_pack(&reads_for(manifest(body)), 1_790_000_000).expect("plan");
    assert!(
        plan.operations
            .iter()
            .any(|op| matches!(op.action, OperationAction::CreatePanel { .. }))
    );
    assert!(
        plan.operations
            .iter()
            .any(|op| matches!(op.action, OperationAction::AddWidget { .. }))
    );
}

#[test]
fn widget_remove_and_configure_are_refused_without_inventory() {
    for op in ["remove", "configure"] {
        let body = format!(
            r#"[capabilities]
requires = ["panel.widgets"]

[[panel.widgets]]
panel = 0
op = "{op}"
type = "org.kde.plasma.digitalclock"
safe = true
"#
        );
        let error = plan_pack(&reads_for(manifest(&body)), 1_790_000_000).expect_err("unplannable");
        assert_eq!(error.code, ErrorCode::UnsupportedResource, "op: {op}");
    }
    let body = r#"[capabilities]
requires = ["panel.widgets"]

[[panel.widgets]]
panel = 0
op = "add"
type = "com.example.evil"
safe = true
"#;
    let error = plan_pack(&reads_for(manifest(body)), 1_790_000_000).expect_err("allowlist");
    assert_eq!(error.code, ErrorCode::UnsupportedResource);
}

#[test]
fn global_theme_expands_to_a_marker_plus_components() {
    let body = r#"[capabilities]
requires = ["appearance.look_and_feel"]

[appearance.look_and_feel]
package = "com.example.mytheme"
components = ["colors", "icons"]
"#;
    let mut reads = reads_for(manifest(body));
    reads
        .look_and_feel_packages
        .push("com.example.mytheme".to_owned());
    let plan = plan_pack(&reads, 1_790_000_000).expect("plan");
    assert_eq!(plan.operations.len(), 1);
    match &plan.operations[0].action {
        OperationAction::ApplyGlobalTheme {
            package_id,
            components,
            expanded,
        } => {
            assert_eq!(package_id, "com.example.mytheme");
            assert_eq!(components, &vec!["colors".to_owned(), "icons".to_owned()]);
            assert!(
                expanded.len() >= 3,
                "marker plus one primitive per component"
            );
        }
        other => panic!("expected ApplyGlobalTheme, got {}", other.name()),
    }
}

#[test]
fn virtual_desktop_add_expands_and_rename_needs_inventory() {
    let body = r#"[capabilities]
requires = ["settings.desktop_behavior"]

[[desktop_behavior.virtual_desktops]]
op = "add"
name = "Work"
"#;
    let plan = plan_pack(&reads_for(manifest(body)), 1_790_000_000).expect("plan");
    assert!(
        plan.operations
            .iter()
            .any(|op| matches!(op.action, OperationAction::ManageVirtualDesktop { .. }))
    );

    let body = r#"[capabilities]
requires = ["settings.desktop_behavior"]

[[desktop_behavior.virtual_desktops]]
op = "rename"
name = "Nope"
"#;
    let mut reads = reads_for(manifest(body));
    reads
        .desktops
        .push(morphix_core::planning::reads::VirtualDesktopView {
            id: None,
            name: "Work".to_owned(),
            position: 0,
        });
    let error = plan_pack(&reads, 1_790_000_000).expect_err("unknown desktop");
    assert_eq!(error.code, ErrorCode::UnsupportedResource);
}

#[test]
fn unmet_dependencies_are_refused_and_applied_ones_warn() {
    let body = format!(
        "{}
[[dependencies.packs]]\nid = \"dev.morphix.base\"\nversion = \">=1.0.0\"\n",
        color_scheme_body()
    );
    let error = plan_pack(&reads_for(manifest(&body)), 1_790_000_000).expect_err("missing");
    assert_eq!(error.code, ErrorCode::DependencyMissing);

    let mut reads = reads_for(manifest(&body));
    reads
        .installed_packs
        .insert("dev.morphix.base".to_owned(), "1.2.0".to_owned());
    reads.applied_packs.push("dev.morphix.base".to_owned());
    let plan = plan_pack(&reads, 1_790_000_000).expect("plan");
    assert!(plan.warnings.iter().any(|warning| matches!(
        warning.kind,
        morphix_core::planning::plan::WarningKind::DependencyApplied
    )));
}

#[test]
fn unsupported_capabilities_are_refused_at_plan_time() {
    let mut reads = reads_for(manifest(&color_scheme_body()));
    reads.capability_states.insert(
        "appearance.color_scheme".to_owned(),
        "unavailable".to_owned(),
    );
    let error = plan_pack(&reads, 1_790_000_000).expect_err("unsupported");
    assert_eq!(error.code, ErrorCode::CapabilityDenied);
}

#[test]
fn service_commands_refuse_honestly_until_later_phases() {
    use morphix_core::platform::paths::{MorphixPaths, XdgPaths};

    let temp = tempfile::tempdir().expect("temp");
    let home = temp.path().join("home").display().to_string();
    let xdg = XdgPaths::from_lookup(|name| (name == "HOME").then(|| home.clone())).expect("xdg");
    let data = temp.path().join("data").display().to_string();
    let state = temp.path().join("state").display().to_string();
    let paths = MorphixPaths::from_xdg_with(&xdg, |name| match name {
        "MORPHIX_DATA_DIR" => Some(data.clone()),
        "MORPHIX_STATE_DIR" => Some(state.clone()),
        _ => None,
    })
    .expect("paths");
    let service = morphix_core::MorphixService::open(paths).expect("service");

    let error = service
        .create_pack_plan("dev.morphix.nope")
        .expect_err("unknown pack");
    assert_eq!(error.code, ErrorCode::InvalidId);

    let error = service
        .create_snapshot_restore_plan("snapshot_missing")
        .expect_err("phase 4 stub");
    assert_eq!(error.code, ErrorCode::UnsupportedResource);

    service
        .discard_plan("plan_missing")
        .expect("discard is idempotent");
}
