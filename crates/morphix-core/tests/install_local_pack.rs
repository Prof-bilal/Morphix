//! Local install, duplicate/conflict, dependency-cycle, and invalid-install tests
//! (P2-11, P2-12, P2-16, AC-18).

mod support;

use std::path::{Path, PathBuf};

use morphix_core::MorphixService;
use morphix_core::core::resource::{PackSource, PackStatus};
use morphix_core::errors::ErrorCode;
use morphix_core::fs::tree;
use morphix_core::platform::paths::{MorphixPaths, XdgPaths};
use morphix_core::storage::db::Storage;
use support::PackBuilder;

fn test_paths(root: &Path) -> MorphixPaths {
    let home = root.join("home").display().to_string();
    let xdg =
        XdgPaths::from_lookup(|name| (name == "HOME").then(|| home.clone())).expect("xdg paths");
    let data = root.join("data").display().to_string();
    let state = root.join("state").display().to_string();
    MorphixPaths::from_xdg_with(&xdg, |name| match name {
        "MORPHIX_DATA_DIR" => Some(data.clone()),
        "MORPHIX_STATE_DIR" => Some(state.clone()),
        _ => None,
    })
    .expect("morphix paths")
}

fn service(root: &Path) -> (MorphixService, MorphixPaths) {
    let paths = test_paths(root);
    let service = MorphixService::open(paths.clone()).expect("service");
    (service, paths)
}

fn builder(root: &Path, name: &str) -> PackBuilder {
    PackBuilder::new(root.join(name))
}

fn with_id(builder: PackBuilder, id: &str) -> PackBuilder {
    builder.replace("dev.example.fixture", id)
}

fn with_dependency(builder: PackBuilder, id: &str) -> PackBuilder {
    builder.append(&format!(
        "\n[[dependencies.packs]]\nid = \"{id}\"\nversion = \">=1.0.0\"\n"
    ))
}

fn packs_hash(paths: &MorphixPaths) -> Option<String> {
    let directory = paths.packs_dir();
    directory.exists().then(|| {
        tree::snapshot(&directory)
            .expect("snapshot packs")
            .content_hash()
    })
}

fn staging_directories(paths: &MorphixPaths) -> usize {
    std::fs::read_dir(paths.packs_dir())
        .map(|entries| {
            entries
                .filter_map(std::result::Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
                .count()
        })
        .unwrap_or(0)
}

fn installed_count(paths: &MorphixPaths) -> usize {
    Storage::open(&paths.database_path())
        .expect("storage")
        .list_installed_packs()
        .expect("installed packs")
        .len()
}

#[test]
fn a_valid_local_pack_is_validated_copied_and_indexed() {
    let temp = tempfile::tempdir().expect("temp");
    let (service, paths) = service(temp.path());
    let source = builder(temp.path(), "source");

    let summary = service
        .install_local_pack(source.build())
        .expect("valid install");

    assert_eq!(summary.id, "dev.example.fixture");
    assert_eq!(summary.source, PackSource::Local);
    assert_eq!(summary.status, PackStatus::Valid);
    assert_eq!(summary.version.as_deref(), Some("1.0.0"));

    let stored = paths.packs_dir().join("dev.example.fixture");
    assert!(stored.join("pack.toml").is_file());
    assert!(stored.join("wallpapers/wall.png").is_file());
    assert_eq!(installed_count(&paths), 1);
    assert_eq!(staging_directories(&paths), 0);

    let detail = service
        .get_pack_detail("dev.example.fixture")
        .expect("detail");
    assert_eq!(detail.assets.len(), 1);
    assert_eq!(detail.assets[0].installed_name, "fixture-wall");
    assert!(
        detail
            .declared_changes
            .iter()
            .any(|change| change.kind == "set_wallpaper")
    );

    let cards = service.list_packs().expect("list");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, "dev.example.fixture");
}

#[test]
fn an_invalid_install_writes_nothing_to_the_pack_store() {
    let temp = tempfile::tempdir().expect("temp");
    let (service, paths) = service(temp.path());
    let source = builder(temp.path(), "broken").remove_file("README.md");

    let error = service
        .install_local_pack(source.build())
        .expect_err("invalid pack is refused");
    assert_eq!(error.code, ErrorCode::RequiredFileMissing);
    assert!(
        error.detail.is_some(),
        "the IPC payload must carry the aggregated issues"
    );

    assert!(!paths.packs_dir().exists(), "no pack store may be created");
    assert_eq!(installed_count(&paths), 0);
    assert!(service.list_packs().expect("list").is_empty());
}

#[test]
fn an_invalid_install_leaves_an_existing_store_byte_identical() {
    let temp = tempfile::tempdir().expect("temp");
    let (service, paths) = service(temp.path());
    let valid = with_id(builder(temp.path(), "valid"), "dev.example.alpha");
    service
        .install_local_pack(valid.build())
        .expect("valid install");

    let before = packs_hash(&paths).expect("pack store exists");
    let broken =
        with_id(builder(temp.path(), "broken"), "dev.example.beta").remove_file("ATTRIBUTION.md");
    let error = service
        .install_local_pack(broken.build())
        .expect_err("invalid pack is refused");
    assert_eq!(error.code, ErrorCode::RequiredFileMissing);

    assert_eq!(packs_hash(&paths).as_deref(), Some(before.as_str()));
    assert_eq!(staging_directories(&paths), 0);
    assert_eq!(installed_count(&paths), 1);
    let cards = service.list_packs().expect("list");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, "dev.example.alpha");
}

#[test]
fn reinstalling_the_same_version_is_refused_without_touching_the_store() {
    let temp = tempfile::tempdir().expect("temp");
    let (service, paths) = service(temp.path());
    let source = builder(temp.path(), "source");
    service
        .install_local_pack(source.build())
        .expect("valid install");
    let before = packs_hash(&paths).expect("pack store exists");

    let error = service
        .install_local_pack(source.build())
        .expect_err("duplicate is refused");
    assert_eq!(error.code, ErrorCode::PackAlreadyInstalled);
    assert_eq!(packs_hash(&paths).as_deref(), Some(before.as_str()));
}

#[test]
fn a_different_version_is_a_conflict_because_v01_does_not_upgrade() {
    let temp = tempfile::tempdir().expect("temp");
    let (service, paths) = service(temp.path());
    let first = builder(temp.path(), "first");
    service
        .install_local_pack(first.build())
        .expect("valid install");
    let before = packs_hash(&paths).expect("pack store exists");

    let upgrade =
        builder(temp.path(), "upgrade").replace("version = \"1.0.0\"", "version = \"2.0.0\"");
    let error = service
        .install_local_pack(upgrade.build())
        .expect_err("upgrade is refused");
    assert_eq!(error.code, ErrorCode::PackIdConflict);
    assert_eq!(packs_hash(&paths).as_deref(), Some(before.as_str()));
    assert_eq!(installed_count(&paths), 1);
}

#[test]
fn a_dependency_cycle_is_refused_at_install_time() {
    let temp = tempfile::tempdir().expect("temp");
    let (service, paths) = service(temp.path());

    let alpha = with_dependency(
        with_id(builder(temp.path(), "alpha"), "dev.example.alpha"),
        "dev.example.beta",
    );
    service
        .install_local_pack(alpha.build())
        .expect("first half of the cycle installs");
    let before = packs_hash(&paths).expect("pack store exists");

    let beta = with_dependency(
        with_id(builder(temp.path(), "beta"), "dev.example.beta"),
        "dev.example.alpha",
    );
    let error = service
        .install_local_pack(beta.build())
        .expect_err("cycle is refused");
    assert_eq!(error.code, ErrorCode::DependencyCycle);
    assert_eq!(packs_hash(&paths).as_deref(), Some(before.as_str()));
    assert_eq!(installed_count(&paths), 1);
}

#[test]
fn the_catalog_lists_installed_bundled_and_invalid_packs() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = test_paths(temp.path());
    let bundled_root: PathBuf = temp.path().join("bundled");
    std::fs::create_dir_all(&bundled_root).expect("bundled root");
    with_id(builder(&bundled_root, "good"), "dev.example.bundled").build();
    let invalid_root = bundled_root.join("invalid");
    std::fs::create_dir_all(&invalid_root).expect("invalid root");
    std::fs::write(invalid_root.join("pack.toml"), b"schema_version = 9\n")
        .expect("invalid manifest");

    let service =
        MorphixService::open_with_bundled_root(paths, Some(bundled_root)).expect("service");
    let installed = with_id(builder(temp.path(), "installed"), "dev.example.local");
    service
        .install_local_pack(installed.build())
        .expect("valid install");

    let cards = service.list_packs().expect("list");
    assert_eq!(cards.len(), 3);
    let by_id = |id: &str| {
        cards
            .iter()
            .find(|card| card.id == id)
            .unwrap_or_else(|| panic!("missing card {id}"))
    };
    assert_eq!(by_id("dev.example.local").status, PackStatus::Valid);
    assert_eq!(by_id("dev.example.bundled").source, PackSource::Bundled);
    assert_eq!(by_id("dev.example.bundled").status, PackStatus::Valid);
    assert_eq!(by_id("invalid").status, PackStatus::Invalid);
    assert!(by_id("invalid").issue_count > 0);

    let detail = service
        .get_pack_detail("dev.example.bundled")
        .expect("detail");
    assert!(!detail.declared_changes.is_empty());
    assert_eq!(
        service
            .get_pack_detail("dev.example.missing")
            .expect_err("missing")
            .code,
        ErrorCode::InvalidId
    );
}
