//! Read-only package manager detection.
//!
//! **Ownership.** Answers "which package manager owns this system?" by checking for the
//! well-known on-disk package databases. It never reads package contents, never opens a
//! database, and never executes a package manager (AC-03, SECURITY.md: no process for
//! detection).
//!
//! # What is checked
//!
//! | Manager | Marker | Applies to |
//! | --- | --- | --- |
//! | pacman | `/var/lib/pacman/local/` (directory) | Arch, Manjaro, EndeavourOS |
//! | dpkg | `/var/lib/dpkg/status` (file) | Debian, Ubuntu, and derivatives |
//! | zypper | `/var/lib/zypp/` (directory) | openSUSE, SLES |
//! | rpm | `/var/lib/rpm/rpmdb.sqlite`, `/var/lib/rpm/Packages`, `/usr/lib/sysimage/rpm/rpmdb.sqlite` | Fedora, RHEL, openSUSE's rpm backend |
//!
//! The marker kind is checked, not just the name: a stray file where a database directory
//! belongs does not produce a false positive. These locations are the package managers'
//! documented data directories; this module makes no claim about other layouts.
//!
//! # Precedence
//!
//! A single distribution can have more than one backend installed (openSUSE has both
//! zypper and rpm; a Debian machine may have rpm installed for cross-packaging). Detection
//! therefore prefers the high-level manager, in this order: pacman, dpkg, zypper, rpm.
//! The result is informational only and never gates apply (MVP.md: distribution and
//! package manager are not support gates).
//!
//! # Honesty
//!
//! Absence of every marker yields [`PackageManager::Unknown`]. Detecting the presence of a
//! database says nothing about whether a package is installed; later phases that need
//! package facts must add a separately reviewed, read-only query.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The package manager Morphix recognizes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageManager {
    /// Arch Linux's `pacman`.
    Pacman,
    /// Debian's `dpkg`.
    Dpkg,
    /// The RPM database.
    Rpm,
    /// openSUSE's `zypper` (an RPM frontend).
    Zypper,
    /// No known package database was found.
    #[default]
    Unknown,
}

impl PackageManager {
    /// Stable lowercase identifier, also used in IPC.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pacman => "pacman",
            Self::Dpkg => "dpkg",
            Self::Rpm => "rpm",
            Self::Zypper => "zypper",
            Self::Unknown => "unknown",
        }
    }

    /// Whether a known manager was detected.
    #[must_use]
    pub fn is_known(self) -> bool {
        self != Self::Unknown
    }
}

impl fmt::Display for PackageManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What kind of filesystem node a marker must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerKind {
    File,
    Directory,
}

/// Ordered detection table. Every path is relative so tests can supply a temporary root.
const MARKERS: &[(PackageManager, &[(&str, MarkerKind)])] = &[
    (
        PackageManager::Pacman,
        &[("var/lib/pacman/local", MarkerKind::Directory)],
    ),
    (
        PackageManager::Dpkg,
        &[("var/lib/dpkg/status", MarkerKind::File)],
    ),
    (
        PackageManager::Zypper,
        &[("var/lib/zypp", MarkerKind::Directory)],
    ),
    (
        PackageManager::Rpm,
        &[
            ("var/lib/rpm/rpmdb.sqlite", MarkerKind::File),
            ("var/lib/rpm/Packages", MarkerKind::File),
            ("usr/lib/sysimage/rpm/rpmdb.sqlite", MarkerKind::File),
        ],
    ),
];

/// Detects the package manager on the running system.
#[must_use]
pub fn detect() -> PackageManager {
    detect_in(Path::new("/"))
}

/// Detects the package manager below a filesystem root.
///
/// The root parameter exists for tests and for a future read-only image inspection; the
/// candidates themselves are fixed and cannot be supplied by a pack (ADR-010).
#[must_use]
pub fn detect_in(root: &Path) -> PackageManager {
    MARKERS
        .iter()
        .find(|(_, markers)| {
            markers
                .iter()
                .any(|(relative, kind)| marker_present(root, relative, *kind))
        })
        .map_or(PackageManager::Unknown, |(manager, _)| *manager)
}

fn marker_present(root: &Path, relative: &str, kind: MarkerKind) -> bool {
    let path = root.join(relative);
    match kind {
        MarkerKind::File => path.is_file(),
        MarkerKind::Directory => path.is_dir(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::path::{Path, PathBuf};

    use super::{PackageManager, detect_in};

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/package-db")
            .join(name)
    }

    #[test]
    fn detects_pacman_from_its_local_database() {
        assert_eq!(detect_in(&fixture("pacman")), PackageManager::Pacman);
    }

    #[test]
    fn detects_dpkg_from_its_status_file() {
        assert_eq!(detect_in(&fixture("dpkg")), PackageManager::Dpkg);
    }

    #[test]
    fn detects_zypper_from_its_state_directory() {
        assert_eq!(detect_in(&fixture("zypper")), PackageManager::Zypper);
    }

    #[test]
    fn detects_all_rpm_database_layouts() {
        assert_eq!(detect_in(&fixture("rpm")), PackageManager::Rpm);
        assert_eq!(detect_in(&fixture("rpm-sysimage")), PackageManager::Rpm);
        assert_eq!(detect_in(&fixture("rpm-bdb")), PackageManager::Rpm);
    }

    #[test]
    fn an_empty_root_is_unknown() {
        assert_eq!(detect_in(&fixture("unknown")), PackageManager::Unknown);
        assert!(!PackageManager::Unknown.is_known());
    }

    #[test]
    fn high_level_managers_win_over_the_rpm_backend() {
        let root = tempfile::tempdir().expect("temp root");

        create_dir(root.path(), "var/lib/rpm");
        write_file(root.path(), "var/lib/rpm/rpmdb.sqlite");
        create_dir(root.path(), "var/lib/zypp");
        assert_eq!(detect_in(root.path()), PackageManager::Zypper);

        create_dir(root.path(), "var/lib/dpkg");
        write_file(root.path(), "var/lib/dpkg/status");
        assert_eq!(detect_in(root.path()), PackageManager::Dpkg);

        create_dir(root.path(), "var/lib/pacman/local");
        assert_eq!(detect_in(root.path()), PackageManager::Pacman);
    }

    #[test]
    fn a_marker_of_the_wrong_kind_is_not_detected() {
        let root = tempfile::tempdir().expect("temp root");

        create_dir(root.path(), "var/lib/pacman");
        write_file(root.path(), "var/lib/pacman/local");
        assert_eq!(detect_in(root.path()), PackageManager::Unknown);

        create_dir(root.path(), "var/lib/dpkg/status");
        assert_eq!(detect_in(root.path()), PackageManager::Unknown);
    }

    #[test]
    fn identifiers_are_stable_and_lowercase() {
        assert_eq!(PackageManager::Pacman.as_str(), "pacman");
        assert_eq!(PackageManager::Dpkg.as_str(), "dpkg");
        assert_eq!(PackageManager::Rpm.as_str(), "rpm");
        assert_eq!(PackageManager::Zypper.as_str(), "zypper");
        assert_eq!(PackageManager::Unknown.as_str(), "unknown");
        assert_eq!(PackageManager::Zypper.to_string(), "zypper");
    }

    #[test]
    fn serde_uses_the_documented_names() {
        for (manager, expected) in [
            (PackageManager::Pacman, "\"pacman\""),
            (PackageManager::Dpkg, "\"dpkg\""),
            (PackageManager::Rpm, "\"rpm\""),
            (PackageManager::Zypper, "\"zypper\""),
            (PackageManager::Unknown, "\"unknown\""),
        ] {
            assert_eq!(
                serde_json::to_string(&manager).expect("serializes"),
                expected
            );
        }
    }

    fn create_dir(root: &Path, relative: &str) {
        std::fs::create_dir_all(root.join(relative)).expect("create fixture dir");
    }

    fn write_file(root: &Path, relative: &str) {
        std::fs::write(root.join(relative), b"marker").expect("create fixture file");
    }
}
