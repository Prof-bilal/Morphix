//! KDE-specific probes.
//!
//! **Ownership.** Reads the presence of the KDE services detection needs, and (P1-09) the
//! Plasma version fallback chain. No writes and no config knowledge.
//!
//! Presence is queried only through [`KdeBus`], so tests never need a session bus.

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::bus::{KdeBus, KdeService};
use crate::core::versions::PlasmaVersion;
use crate::errors::Result;
use crate::platform::package_db::PackageManager;
use crate::platform::probe::{CommandRunner, VersionProbe};

/// Which KDE services are currently running in this session.
///
/// A missing service is a fact, not an error. An error from the bus means the question
/// could not be answered and propagates as `E_ADAPTER_UNAVAILABLE`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdePresence {
    /// `org.kde.plasmashell` has an owner.
    pub plasmashell: bool,
    /// `org.kde.KWin` has an owner.
    pub kwin: bool,
    /// `org.kde.kglobalaccel` has an owner.
    pub kglobalaccel: bool,
}

impl KdePresence {
    /// Whether a Plasma shell is running. AC-02 calls this `plasmashell_running`.
    #[must_use]
    pub fn plasmashell_running(self) -> bool {
        self.plasmashell
    }

    /// Whether all three services responded with an owner.
    #[must_use]
    pub fn all_present(self) -> bool {
        self.plasmashell && self.kwin && self.kglobalaccel
    }

    /// The services that are not running, in probing order.
    #[must_use]
    pub fn missing(self) -> Vec<KdeService> {
        let mut missing = Vec::new();
        if !self.plasmashell {
            missing.push(KdeService::PlasmaShell);
        }
        if !self.kwin {
            missing.push(KdeService::KWin);
        }
        if !self.kglobalaccel {
            missing.push(KdeService::KGlobalAccel);
        }
        missing
    }
}

/// The KDE configuration files detection requires to be readable.
///
/// Only presence/readability is reported; contents are never read here. Write support for
/// these files belongs to the Phase 6 target map.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdeConfigPresence {
    /// `kdeglobals` is an existing, readable regular file.
    pub kdeglobals: bool,
    /// `kwinrc` is an existing, readable regular file.
    pub kwinrc: bool,
}

/// The KDE config files whose readability the support gate requires.
pub const KDEGLOBALS_FILE: &str = "kdeglobals";
/// The KWin config file whose readability the support gate requires.
pub const KWINRC_FILE: &str = "kwinrc";

/// Checks `kdeglobals` and `kwinrc` under the given XDG config directory.
///
/// Read-only: opening the file proves it is readable, and does not parse or log contents.
/// Symlinks are followed like any other read; refusing symlinked *writes* is a separate
/// rule that applies to the write path.
#[must_use]
pub fn probe_config_presence(config_home: &Path) -> KdeConfigPresence {
    KdeConfigPresence {
        kdeglobals: is_readable_file(&config_home.join(KDEGLOBALS_FILE)),
        kwinrc: is_readable_file(&config_home.join(KWINRC_FILE)),
    }
}

fn is_readable_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
        && std::fs::File::open(path).is_ok()
}

/// Queries the three KDE service names through the supplied bus.
///
/// # Errors
///
/// Propagates any `KdeBus` error (mapped to `E_ADAPTER_UNAVAILABLE` by the real bus). A
/// missing owner is returned as `false`, never as an error.
pub fn probe_presence(bus: &dyn KdeBus) -> Result<KdePresence> {
    Ok(KdePresence {
        plasmashell: bus.has_owner(KdeService::PlasmaShell)?,
        kwin: bus.has_owner(KdeService::KWin)?,
        kglobalaccel: bus.has_owner(KdeService::KGlobalAccel)?,
    })
}

// ---------------------------------------------------------------------------
// Plasma version fallback chain
// ---------------------------------------------------------------------------

/// Package names that carry the Plasma release version.
///
/// `plasma-workspace` owns `plasmashell` on Arch, Debian, and Fedora; openSUSE prefixes the
/// Plasma 6 packages with `plasma6-`.
const PLASMA_PACKAGE_NAMES: &[&str] = &["plasma-workspace", "plasma6-workspace"];

/// Pacman's local database, relative to the filesystem root.
pub const PACMAN_LOCAL_DB: &str = "var/lib/pacman/local";

/// dpkg's status database, relative to the filesystem root.
pub const DPKG_STATUS_DB: &str = "var/lib/dpkg/status";

/// A `desc` file is a few KiB; 1 MiB is a hard refusal, not a buffer size.
const PACMAN_DESC_CAP: u64 = 1024 * 1024;

/// A large dpkg status file is still well under this; the cap only bounds a corrupt file.
const DPKG_STATUS_CAP: u64 = 64 * 1024 * 1024;

/// Detects the Plasma version, trying the read-only package database first and the
/// allowlisted version probes second.
///
/// Returns `None` when every source is missing, unreadable, or unparseable. It never
/// substitutes a default version: the support verdict must be able to say "unknown".
///
/// # Why rpm/zypper have no package-database step
///
/// Their databases are SQLite/Berkeley DB. Querying them needs SQL, and SQL is owned by
/// `storage` (AGENTS.md §13); parsing a binary RPM database in `desktop` would violate that
/// boundary. The fixed-argv probe is the documented fallback and works on every distro, so
/// Phase 1 falls through to it there.
#[must_use]
pub fn detect_plasma_version(
    package_manager: PackageManager,
    root: &Path,
    runner: &CommandRunner,
) -> Option<PlasmaVersion> {
    detect_with(package_manager, root, || probe_plasma_version(runner))
}

fn detect_with(
    package_manager: PackageManager,
    root: &Path,
    probe: impl FnOnce() -> Option<PlasmaVersion>,
) -> Option<PlasmaVersion> {
    let from_package_db = match package_manager {
        PackageManager::Pacman => pacman_plasma_version(root),
        PackageManager::Dpkg => dpkg_plasma_version(root),
        PackageManager::Rpm | PackageManager::Zypper | PackageManager::Unknown => None,
    };

    from_package_db.or_else(probe)
}

/// Runs `plasmashell --version`, falling back to `kwin_wayland --version`.
///
/// Both are SECURITY.md §7 probes with fixed argv and a 2 s timeout. A probe that does not
/// produce the expected one-line shape is discarded, not mined for a number.
#[must_use]
pub fn probe_plasma_version(runner: &CommandRunner) -> Option<PlasmaVersion> {
    for probe in [VersionProbe::PlasmaShell, VersionProbe::KWinWayland] {
        let Ok(output) = runner.run(probe) else {
            continue;
        };
        if let Some(version) = parse_probe_version(probe, &output) {
            return Some(version);
        }
    }
    None
}

/// Parses one version-probe line.
///
/// Accepted shapes are exactly `plasmashell <version>` and `kwin <version>` (the current
/// output of each binary, including the short `kwin` name on Plasma 6.8). Any extra token,
/// a different name, or a non-version value yields `None`.
#[must_use]
pub fn parse_probe_version(probe: VersionProbe, output: &str) -> Option<PlasmaVersion> {
    let mut tokens = output.split_whitespace();
    let name = tokens.next()?;
    let version = tokens.next()?;
    if tokens.next().is_some() || name != probe_name(probe) {
        return None;
    }
    PlasmaVersion::parse(version).ok()
}

fn probe_name(probe: VersionProbe) -> &'static str {
    match probe {
        VersionProbe::PlasmaShell => "plasmashell",
        VersionProbe::KWinWayland => "kwin",
    }
}

/// Reads the pacman local database for a Plasma package.
///
/// Each installed package is a directory named `<name>-<pkgver>-<pkgrel>` with a `desc`
/// file. The `%VERSION%` field includes the pkgrel (`6.8.0-1.1`), which is stripped before
/// SemVer parsing; the package release is not part of the Plasma version.
fn pacman_plasma_version(root: &Path) -> Option<PlasmaVersion> {
    let local_db = root.join(PACMAN_LOCAL_DB);
    let entries = std::fs::read_dir(local_db).ok()?;

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let directory = file_name.to_string_lossy();

        for package in PLASMA_PACKAGE_NAMES {
            let Some(rest) = directory
                .strip_prefix(package)
                .and_then(|rest| rest.strip_prefix('-'))
            else {
                continue;
            };
            if !rest.starts_with(|character: char| character.is_ascii_digit()) {
                continue;
            }

            let Some(desc) = read_capped(&entry.path().join("desc"), PACMAN_DESC_CAP) else {
                continue;
            };
            if pacman_field(&desc, "NAME") != Some(*package) {
                continue;
            }
            let Some(version) = pacman_field(&desc, "VERSION") else {
                continue;
            };
            if let Ok(version) = PlasmaVersion::parse(pacman_upstream(version)) {
                return Some(version);
            }
        }
    }

    None
}

/// Reads one field from a pacman `desc` file.
fn pacman_field<'a>(desc: &'a str, field: &str) -> Option<&'a str> {
    let marker = format!("%{field}%");
    let mut lines = desc.lines();
    while let Some(line) = lines.next() {
        if line.trim() == marker {
            return lines
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty());
        }
    }
    None
}

/// Pacman's `%VERSION%` is `<pkgver>-<pkgrel>`; the pkgrel is not a pre-release.
fn pacman_upstream(version: &str) -> &str {
    version
        .rsplit_once('-')
        .map_or(version, |(upstream, _pkgrel)| upstream)
}

/// Reads dpkg's status database for a Plasma package.
///
/// Stanzas are separated by a blank line. `Version` may carry an epoch (`4:`) and a Debian
/// revision (`-1ubuntu1`); both are stripped before SemVer parsing. A version with a tilde
/// or other non-SemVer suffix is left for the probe fallback rather than guessed at.
fn dpkg_plasma_version(root: &Path) -> Option<PlasmaVersion> {
    let status = read_capped(&root.join(DPKG_STATUS_DB), DPKG_STATUS_CAP)?;

    for stanza in status.split("\n\n") {
        let mut package = None;
        let mut version = None;
        for line in stanza.lines() {
            if let Some(value) = line.strip_prefix("Package:") {
                package = Some(value.trim());
            } else if let Some(value) = line.strip_prefix("Version:") {
                version = Some(value.trim());
            }
        }

        let (Some(package), Some(version)) = (package, version) else {
            continue;
        };
        if !PLASMA_PACKAGE_NAMES.contains(&package) {
            continue;
        }
        if let Ok(version) = PlasmaVersion::parse(debian_upstream(version)) {
            return Some(version);
        }
    }

    None
}

/// dpkg's version is `[epoch:]upstream[-revision]`.
fn debian_upstream(version: &str) -> &str {
    let without_epoch = version
        .split_once(':')
        .map_or(version, |(_epoch, upstream)| upstream);
    without_epoch
        .rsplit_once('-')
        .map_or(without_epoch, |(upstream, _revision)| upstream)
}

/// Reads a file with a hard size ceiling; any failure (including oversize) is `None`.
///
/// Package databases are system input: they must not be able to allocate without bound or
/// abort detection with an I/O error.
fn read_capped(path: &Path, cap: u64) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut limited = file.take(cap.saturating_add(1));
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > cap {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::cell::Cell;
    use std::path::{Path, PathBuf};

    use super::{
        KdePresence, debian_upstream, detect_with, pacman_upstream, parse_probe_version,
        probe_config_presence, probe_presence,
    };
    use crate::core::versions::PlasmaVersion;
    use crate::desktop::kde::bus::{FakeBus, KdeService};
    use crate::errors::{ErrorCode, MorphixError};
    use crate::platform::package_db::PackageManager;
    use crate::platform::probe::VersionProbe;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/plasma-version")
            .join(name)
    }

    fn version(text: &str) -> PlasmaVersion {
        PlasmaVersion::parse(text).expect("valid fixture version")
    }

    #[test]
    fn all_running_services_are_reported_present() {
        let bus = FakeBus::all_present();
        let presence = probe_presence(&bus).expect("probe succeeds");

        assert_eq!(
            presence,
            KdePresence {
                plasmashell: true,
                kwin: true,
                kglobalaccel: true,
            }
        );
        assert!(presence.plasmashell_running());
        assert!(presence.all_present());
        assert!(presence.missing().is_empty());
        assert_eq!(
            bus.calls(),
            [
                KdeService::PlasmaShell,
                KdeService::KWin,
                KdeService::KGlobalAccel
            ]
        );
    }

    #[test]
    fn absent_services_are_false_not_an_error() {
        let presence = probe_presence(&FakeBus::new()).expect("probe succeeds");

        assert!(!presence.plasmashell_running());
        assert!(!presence.all_present());
        assert_eq!(
            presence.missing(),
            [
                KdeService::PlasmaShell,
                KdeService::KWin,
                KdeService::KGlobalAccel
            ]
        );
    }

    #[test]
    fn a_partial_session_is_reported_per_service() {
        let bus = FakeBus::new().with_owner(KdeService::PlasmaShell);
        let presence = probe_presence(&bus).expect("probe succeeds");

        assert!(presence.plasmashell_running());
        assert!(!presence.kwin);
        assert!(!presence.kglobalaccel);
        assert_eq!(
            presence.missing(),
            [KdeService::KWin, KdeService::KGlobalAccel]
        );
    }

    #[test]
    fn a_bus_error_is_not_mistaken_for_an_absent_service() {
        let error = MorphixError::new(ErrorCode::AdapterUnavailable, "bus failed");
        let bus = FakeBus::failing(error);

        let returned = probe_presence(&bus).expect_err("bus error propagates");
        assert_eq!(returned.code, ErrorCode::AdapterUnavailable);
        assert_eq!(bus.calls().len(), 1, "the first query stops the probe");
    }

    #[test]
    fn config_presence_requires_readable_regular_files() {
        let home = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            probe_config_presence(home.path()),
            super::KdeConfigPresence::default(),
            "an empty config dir has neither file"
        );

        std::fs::write(home.path().join(super::KDEGLOBALS_FILE), "[General]\n")
            .expect("write kdeglobals");
        std::fs::write(home.path().join(super::KWINRC_FILE), "[Compositing]\n")
            .expect("write kwinrc");

        assert_eq!(
            probe_config_presence(home.path()),
            super::KdeConfigPresence {
                kdeglobals: true,
                kwinrc: true,
            }
        );
    }

    #[test]
    fn a_directory_named_like_a_config_file_is_not_presence() {
        let home = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(home.path().join(super::KDEGLOBALS_FILE)).expect("create directory");
        std::fs::write(home.path().join(super::KWINRC_FILE), "[Compositing]\n")
            .expect("write kwinrc");

        let presence = probe_config_presence(home.path());
        assert!(!presence.kdeglobals);
        assert!(presence.kwinrc);
    }

    #[test]
    fn config_presence_performs_no_writes() {
        let home = tempfile::tempdir().expect("tempdir");
        let _ = probe_config_presence(home.path());

        assert_eq!(
            std::fs::read_dir(home.path())
                .expect("read config dir")
                .count(),
            0
        );
    }

    #[test]
    fn presence_serializes_as_plain_booleans_for_ipc() {
        let presence = KdePresence {
            plasmashell: true,
            kwin: false,
            kglobalaccel: true,
        };
        assert_eq!(
            serde_json::to_value(presence).expect("serializes"),
            serde_json::json!({
                "plasmashell": true,
                "kwin": false,
                "kglobalaccel": true
            })
        );
    }

    #[test]
    fn probe_output_is_parsed_only_in_the_documented_shape() {
        let accepted = [
            (VersionProbe::PlasmaShell, "plasmashell 6.8.0", "6.8.0"),
            (VersionProbe::PlasmaShell, "  plasmashell   6.8  ", "6.8.0"),
            (VersionProbe::KWinWayland, "kwin 6.8.0", "6.8.0"),
        ];
        for (probe, output, expected) in accepted {
            assert_eq!(
                parse_probe_version(probe, output),
                Some(version(expected)),
                "{output:?}"
            );
        }

        let rejected = [
            (VersionProbe::PlasmaShell, "plasmashell 6.8.0 extra"),
            (VersionProbe::PlasmaShell, "globalshortcuts 6.8.0"),
            (VersionProbe::PlasmaShell, "plasmashell"),
            (VersionProbe::PlasmaShell, "6.8.0"),
            (VersionProbe::PlasmaShell, "plasmashell not-a-version"),
            (VersionProbe::KWinWayland, "kwin_wayland 6.8.0"),
        ];
        for (probe, output) in rejected {
            assert_eq!(parse_probe_version(probe, output), None, "{output:?}");
        }
    }

    #[test]
    fn package_release_suffixes_are_not_versions() {
        assert_eq!(pacman_upstream("6.8.0-1.1"), "6.8.0");
        assert_eq!(pacman_upstream("6.8.0"), "6.8.0");
        assert_eq!(debian_upstream("4:6.3.5-1"), "6.3.5");
        assert_eq!(debian_upstream("6.3.5-1ubuntu1"), "6.3.5");
        assert_eq!(debian_upstream("6.3.5"), "6.3.5");
    }

    #[test]
    fn pacman_version_strips_the_package_release() {
        let probe_called = Cell::new(false);
        let detected = detect_with(PackageManager::Pacman, &fixture("pacman"), || {
            probe_called.set(true);
            None
        });

        assert_eq!(detected, Some(version("6.8.0")));
        assert!(!probe_called.get(), "the package database is preferred");
    }

    #[test]
    fn dpkg_version_strips_epoch_and_debian_revision() {
        let detected = detect_with(PackageManager::Dpkg, &fixture("dpkg"), || None);
        assert_eq!(detected, Some(version("6.3.5")));
    }

    #[test]
    fn the_plasma6_package_name_is_recognized() {
        let detected = detect_with(PackageManager::Pacman, &fixture("pacman-opensuse"), || None);
        assert_eq!(detected, Some(version("6.9.0")));
    }

    #[test]
    fn the_probe_runs_when_the_package_database_has_no_answer() {
        let from_unknown_manager =
            detect_with(PackageManager::Unknown, &fixture("missing"), || {
                Some(version("6.4.1"))
            });
        assert_eq!(from_unknown_manager, Some(version("6.4.1")));

        let from_missing_database =
            detect_with(PackageManager::Pacman, &fixture("missing"), || {
                Some(version("6.4.1"))
            });
        assert_eq!(from_missing_database, Some(version("6.4.1")));
    }

    #[test]
    fn an_unreadable_source_reports_none_rather_than_guessing() {
        assert_eq!(
            detect_with(PackageManager::Pacman, &fixture("missing"), || None),
            None
        );
    }

    #[test]
    fn a_non_semver_package_version_falls_back_to_the_probe() {
        let root = tempfile::tempdir().expect("tempdir");
        let directory = root
            .path()
            .join("var/lib/pacman/local/plasma-workspace-6.8.0~rc1-1");
        std::fs::create_dir_all(&directory).expect("create package dir");
        std::fs::write(
            directory.join("desc"),
            "%NAME%\nplasma-workspace\n\n%VERSION%\n6.8.0~rc1-1\n",
        )
        .expect("write desc");

        let detected = detect_with(PackageManager::Pacman, root.path(), || {
            Some(version("6.7.0"))
        });
        assert_eq!(detected, Some(version("6.7.0")));
    }

    #[test]
    fn an_oversized_package_database_entry_is_ignored() {
        let root = tempfile::tempdir().expect("tempdir");
        let directory = root
            .path()
            .join("var/lib/pacman/local/plasma-workspace-6.8.0-1");
        std::fs::create_dir_all(&directory).expect("create package dir");
        std::fs::write(directory.join("desc"), vec![b'x'; 1024 * 1024 + 1])
            .expect("write oversized desc");

        let detected = detect_with(PackageManager::Pacman, root.path(), || {
            Some(version("6.7.0"))
        });
        assert_eq!(detected, Some(version("6.7.0")));
    }
}
