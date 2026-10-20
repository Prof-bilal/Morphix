//! P1-13: detection is read-only except for the data-directory write probe, which must
//! create and remove its own file.
//!
//! The suite runs every injected detection path against a temporary root and snapshots the
//! tree before and after. It also checks that the support and status views agree and that
//! no mutation is implied by a supported verdict.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use morphix_core::core::versions::PlasmaVersion;
use morphix_core::desktop::kde::{
    FakeBus, KdeConfigPresence, KdePresence, detect_plasma_version, probe_config_presence,
    probe_presence,
};
use morphix_core::platform::os_release::OsRelease;
use morphix_core::platform::package_db::{self, PackageManager};
use morphix_core::platform::probe::CommandRunner;
use morphix_core::platform::session::{SessionInfo, SessionType};
use morphix_core::system::{
    Architecture, DetectionInputs, SupportInputs, SupportLevel, assemble_status, evaluate,
    probe_data_dir_writable,
};

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write fixture");
}

/// Every file under `root`, keyed by relative path, with its exact bytes.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}

fn collect(base: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in fs::read_dir(directory).expect("read directory") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).expect("metadata");

        if metadata.is_dir() {
            collect(base, &path, files);
        } else {
            files.insert(
                path.strip_prefix(base)
                    .expect("relative path")
                    .to_path_buf(),
                fs::read(&path).expect("read file"),
            );
        }
    }
}

#[test]
fn every_injected_detection_path_leaves_the_tree_unchanged() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let root = temporary.path();

    write(
        root,
        "etc/os-release",
        "ID=arch\nNAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nVERSION_ID=rolling\nID_LIKE=arch\n",
    );
    write(
        root,
        "var/lib/pacman/local/plasma-workspace-6.8.0-1.1/desc",
        "%NAME%\nplasma-workspace\n\n%VERSION%\n6.8.0-1.1\n",
    );
    write(
        root,
        "home/.config/kdeglobals",
        "[General]\nColorScheme=Breeze\n",
    );
    write(root, "home/.config/kwinrc", "[Compositing]\nEnabled=true\n");
    fs::create_dir_all(root.join("home/.local/share/morphix")).expect("create data dir");

    let before = snapshot(root);

    let release = OsRelease::from_file(root.join("etc/os-release")).expect("os-release");
    let manager = package_db::detect_in(root);
    let version = detect_plasma_version(manager, root, &CommandRunner::new());
    let config = probe_config_presence(&root.join("home/.config"));
    let presence = probe_presence(&FakeBus::all_present()).expect("fake bus");

    let session = SessionInfo::new(1000, 1000, SessionType::Wayland);
    let data_dir_writable = probe_data_dir_writable(&root.join("home/.local/share/morphix"));

    let status = assemble_status(DetectionInputs {
        linux: true,
        os_release: release,
        package_manager: manager,
        session,
        session_bus: true,
        plasma: presence,
        plasma_version: version.clone(),
        kde_config: config,
        data_dir_writable,
        architecture: Architecture::X86_64,
        screen_count: Some(1),
    });

    let report = evaluate(&SupportInputs {
        linux: true,
        session_bus: true,
        plasma: KdePresence {
            plasmashell: true,
            kwin: true,
            kglobalaccel: true,
        },
        plasma_version: version,
        kde_config: KdeConfigPresence {
            kdeglobals: true,
            kwinrc: true,
        },
        session,
        data_dir_writable,
        architecture: Architecture::X86_64,
        screen_count: Some(1),
    });

    let after = snapshot(root);

    assert_eq!(manager, PackageManager::Pacman);
    assert_eq!(
        status.plasma_version,
        Some(PlasmaVersion::parse("6.8.0").expect("version"))
    );
    assert_eq!(status.support, report);
    assert_eq!(status.support.level, SupportLevel::Supported);
    assert!(status.supported);
    assert!(
        status.support.reasons.is_empty(),
        "a supported verdict has no reasons"
    );
    assert_eq!(
        before, after,
        "detection changed the filesystem tree, the data-dir probe file was not removed, \
         or a probe wrote outside its allowed path"
    );
}

#[test]
fn the_data_dir_probe_removes_its_file_even_when_the_directory_is_reused() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let data_dir = temporary.path().join("morphix");
    fs::create_dir_all(&data_dir).expect("create data dir");
    fs::write(data_dir.join("existing"), b"keep me").expect("existing file");

    let before = snapshot(&data_dir);
    assert!(probe_data_dir_writable(&data_dir));
    assert!(probe_data_dir_writable(&data_dir));
    let after = snapshot(&data_dir);

    assert_eq!(before, after);
    assert_eq!(after.len(), 1, "only the pre-existing file remains");
}

#[test]
fn detection_returns_a_verdict_without_creating_a_mutation_journal() {
    // Detection has no transaction dependency; a supported status is data, not a change.
    // The architecture test pins the dependency edge, and this asserts the observable
    // shape: no transaction identifier or applied marker is present in the DTO.
    let temporary = tempfile::tempdir().expect("tempdir");
    let root = temporary.path();

    let status = assemble_status(DetectionInputs {
        linux: true,
        os_release: OsRelease::parse("ID=arch\nPRETTY_NAME=\"Arch Linux\"\n"),
        package_manager: PackageManager::Pacman,
        session: SessionInfo::new(1000, 1000, SessionType::Wayland),
        session_bus: true,
        plasma: KdePresence {
            plasmashell: true,
            kwin: true,
            kglobalaccel: true,
        },
        plasma_version: Some(PlasmaVersion::parse("6.8.0").expect("version")),
        kde_config: KdeConfigPresence {
            kdeglobals: true,
            kwinrc: true,
        },
        data_dir_writable: true,
        architecture: Architecture::X86_64,
        screen_count: Some(1),
    });

    let value = serde_json::to_value(status).expect("serializes");
    assert!(value.get("transactionId").is_none());
    assert!(value.get("applied").is_none());
    assert_eq!(snapshot(root).len(), 0);
}
