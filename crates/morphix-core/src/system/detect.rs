//! System detection: collect facts, build [`SystemStatus`], and cache the result.
//!
//! **Ownership.** The one place that combines OS, session, package-database, KDE DBus, and
//! KDE config facts. It performs reads only, with one documented exception: the data-dir
//! writability probe creates and immediately removes a zero-byte file in Morphix's own
//! data directory (MVP.md §1). Nothing outside that directory is written.
//!
//! **Dependencies.** `platform`, `desktop`, and `errors`. No `packs`, `planning`,
//! `transactions`, or `storage`.
//!
//! Detection runs on a blocking worker. The Tauri command wraps [`SystemStatusCache`] and
//! calls [`detect`] through `spawn_blocking`.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::core::versions::PlasmaVersion;
use crate::desktop::kde::{
    KdeConfigPresence, KdePresence, SessionBus, detect_plasma_version, probe_config_presence,
    probe_presence,
};
use crate::errors::Result;
use crate::platform::os_release::{self, OsRelease};
use crate::platform::package_db::{self, PackageManager};
use crate::platform::paths::{MorphixPaths, XdgPaths};
use crate::platform::probe::CommandRunner;
use crate::platform::session::{self, SessionInfo, SessionType};

use super::support::{Architecture, SupportInputs, SupportReport, evaluate};

/// Cached status lifetime.
///
/// Long enough that navigating between screens does not re-run DBus and probe commands,
/// short enough that a fixed session or a new Plasma version is noticed without a restart.
pub const SYSTEM_STATUS_TTL: Duration = Duration::from_secs(30);

/// Distribution identity in the IPC shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistroInfo {
    /// `ID`, for example `arch`.
    pub id: Option<String>,
    /// `NAME`.
    pub name: Option<String>,
    /// `PRETTY_NAME`, the preferred display label.
    pub pretty_name: Option<String>,
    /// `VERSION_ID`.
    pub version_id: Option<String>,
    /// `ID_LIKE`, in file order.
    pub id_like: Vec<String>,
}

impl From<&OsRelease> for DistroInfo {
    fn from(release: &OsRelease) -> Self {
        Self {
            id: release.id.clone(),
            name: release.name.clone(),
            pretty_name: release.pretty_name.clone(),
            version_id: release.version_id.clone(),
            id_like: release.id_like.clone(),
        }
    }
}

/// The status object returned by `get_system_status` (MVP.md AC-01…AC-04).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    /// Convenience mirror of `support.level == supported` for the status card.
    pub supported: bool,
    /// Support level, reasons, and summary.
    pub support: SupportReport,
    /// Distribution identity; every field may be unknown.
    pub distro: DistroInfo,
    /// Detected package manager, informational and never a gate (AC-03).
    pub package_manager: PackageManager,
    /// Detected Plasma version, if any source produced one.
    pub plasma_version: Option<PlasmaVersion>,
    /// `org.kde.plasmashell` presence (AC-02).
    pub plasmashell_running: bool,
    /// Wayland, X11, TTY, or unknown.
    pub session_type: SessionType,
}

/// Facts collected by [`detect`], separated so the mapping can be unit-tested without I/O.
#[derive(Debug, Clone)]
pub struct DetectionInputs {
    /// `/proc` and an os-release file are available.
    pub linux: bool,
    /// Parsed distribution identity.
    pub os_release: OsRelease,
    /// Read-only package-manager detection result.
    pub package_manager: PackageManager,
    /// Process UID and session type.
    pub session: SessionInfo,
    /// The session bus accepted a connection.
    pub session_bus: bool,
    /// KDE service presence; all false when there is no bus.
    pub plasma: KdePresence,
    /// Plasma version from the package database or a version probe.
    pub plasma_version: Option<PlasmaVersion>,
    /// Readability of `kdeglobals` and `kwinrc`.
    pub kde_config: KdeConfigPresence,
    /// The data-directory write probe succeeded.
    pub data_dir_writable: bool,
    /// Compiled target architecture.
    pub architecture: Architecture,
    /// Known screen count; `None` when not probed.
    pub screen_count: Option<u32>,
}

/// Builds the DTO from already-collected facts. Pure: no I/O, no writes.
#[must_use]
pub fn assemble_status(inputs: DetectionInputs) -> SystemStatus {
    let support = evaluate(&SupportInputs {
        linux: inputs.linux,
        session_bus: inputs.session_bus,
        plasma: inputs.plasma,
        plasma_version: inputs.plasma_version.clone(),
        kde_config: inputs.kde_config,
        session: inputs.session,
        data_dir_writable: inputs.data_dir_writable,
        architecture: inputs.architecture,
        screen_count: inputs.screen_count,
    });

    SystemStatus {
        supported: support.is_supported(),
        support,
        distro: DistroInfo::from(&inputs.os_release),
        package_manager: inputs.package_manager,
        plasma_version: inputs.plasma_version,
        plasmashell_running: inputs.plasma.plasmashell,
        session_type: inputs.session.session_type,
    }
}

/// Detects the current system.
///
/// # Errors
///
/// Returns a typed error when the process identity cannot be determined (the root check
/// must fail closed), the XDG locations are unusable, or the session bus answered a
/// presence query with an error. A missing session bus is a normal `NO_SESSION_BUS`
/// verdict, not an error.
pub fn detect() -> Result<SystemStatus> {
    let xdg = XdgPaths::from_env()?;
    let paths = MorphixPaths::from_env()?;

    let os_release_result = OsRelease::from_system();
    let linux = Path::new("/proc").is_dir()
        && (Path::new(os_release::SYSTEM_OS_RELEASE).is_file()
            || Path::new(os_release::FALLBACK_OS_RELEASE).is_file())
        && os_release_result.is_ok();

    let os_release = match os_release_result {
        Ok(release) => release,
        Err(error) => {
            tracing::warn!(error = %error, "failed to read os-release; distribution is unknown");
            OsRelease::default()
        }
    };

    if !linux {
        // Do not touch /proc or DBus: the gate is already decided. The placeholder session
        // is never consulted because evaluate() short-circuits on `linux == false`.
        return Ok(assemble_status(DetectionInputs {
            linux: false,
            os_release,
            package_manager: PackageManager::Unknown,
            session: SessionInfo::new(1, 1, SessionType::Unknown),
            session_bus: false,
            plasma: KdePresence::default(),
            plasma_version: None,
            kde_config: KdeConfigPresence::default(),
            data_dir_writable: false,
            architecture: Architecture::current(),
            screen_count: None,
        }));
    }

    let session = session::probe()?;
    let package_manager = package_db::detect();

    let runner = CommandRunner::new();
    let plasma_version = detect_plasma_version(package_manager, Path::new("/"), &runner);

    let bus = match SessionBus::connect() {
        Ok(bus) => Some(bus),
        Err(error) => {
            tracing::debug!(error = %error, "session bus unavailable; reporting NO_SESSION_BUS");
            None
        }
    };
    let session_bus = bus.is_some();
    let plasma = match bus.as_ref() {
        Some(bus) => probe_presence(bus)?,
        None => KdePresence::default(),
    };

    let kde_config = probe_config_presence(xdg.config_home());
    let data_dir_writable = probe_data_dir_writable(paths.data_dir());

    let status = assemble_status(DetectionInputs {
        linux: true,
        os_release,
        package_manager,
        session,
        session_bus,
        plasma,
        plasma_version,
        kde_config,
        data_dir_writable,
        architecture: Architecture::current(),
        screen_count: None,
    });

    tracing::info!(
        level = status.support.level.as_str(),
        reasons = ?status.support.reasons,
        session_type = status.session_type.as_str(),
        plasmashell_running = status.plasmashell_running,
        "system support detected"
    );

    Ok(status)
}

/// Creates and removes a zero-byte probe file in Morphix's data directory.
///
/// This is the only write detection performs. The file is created with `create_new` so an
/// existing path is never truncated, and it is removed immediately. A failure to remove it
/// is logged but does not make the directory unwritable; a leftover zero-byte file is
/// harmless and is not a mutation of user state.
#[must_use]
pub fn probe_data_dir_writable(data_dir: &Path) -> bool {
    if std::fs::create_dir_all(data_dir).is_err() {
        return false;
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let probe = data_dir.join(format!(
        ".morphix-write-probe-{}-{nonce}",
        std::process::id()
    ));

    let created = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe);

    match created {
        Ok(file) => {
            drop(file);
            if let Err(error) = std::fs::remove_file(&probe) {
                tracing::warn!(
                    error = %error,
                    path = %probe.display(),
                    "failed to remove the data-directory write probe"
                );
            }
            true
        }
        Err(_) => false,
    }
}

/// Short-lived, thread-safe cache for [`SystemStatus`].
///
/// Concurrent callers may both detect; detection is read-only, so the duplicate work is
/// preferable to holding a lock across DBus and probe calls.
#[derive(Debug)]
pub struct SystemStatusCache {
    ttl: Duration,
    cached: Mutex<Option<CachedStatus>>,
}

#[derive(Debug)]
struct CachedStatus {
    status: SystemStatus,
    captured_at: Instant,
}

impl SystemStatusCache {
    /// Creates a cache with the given lifetime.
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            cached: Mutex::new(None),
        }
    }

    /// Returns the cached status when it is fresh and `refresh` is false, otherwise runs
    /// `detect` and stores the result.
    ///
    /// # Errors
    ///
    /// Propagates `detect`'s error. A failed detection is never cached, so the next call
    /// retries instead of pinning a transient failure.
    pub fn get_or_detect<F>(&self, refresh: bool, detect: F) -> Result<SystemStatus>
    where
        F: FnOnce() -> Result<SystemStatus>,
    {
        if !refresh {
            let cached = self.lock();
            if let Some(cached) = cached.as_ref()
                && cached.captured_at.elapsed() < self.ttl
            {
                return Ok(cached.status.clone());
            }
        }

        let status = detect()?;

        let mut cached = self.lock();
        *cached = Some(CachedStatus {
            status: status.clone(),
            captured_at: Instant::now(),
        });

        Ok(status)
    }

    fn lock(&self) -> MutexGuard<'_, Option<CachedStatus>> {
        self.cached.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::cell::Cell;
    use std::time::Duration;

    use super::{
        DetectionInputs, SystemStatus, SystemStatusCache, assemble_status, probe_data_dir_writable,
    };
    use crate::core::versions::PlasmaVersion;
    use crate::desktop::kde::{KdeConfigPresence, KdePresence};
    use crate::errors::{ErrorCode, MorphixError};
    use crate::platform::os_release::OsRelease;
    use crate::platform::package_db::PackageManager;
    use crate::platform::session::{SessionInfo, SessionType};
    use crate::system::support::{Architecture, SupportLevel, SupportReason};

    fn version(text: &str) -> PlasmaVersion {
        PlasmaVersion::parse(text).expect("valid version")
    }

    fn baseline_inputs() -> DetectionInputs {
        DetectionInputs {
            linux: true,
            os_release: OsRelease::parse(
                "ID=arch\nNAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\n\
                 VERSION_ID=rolling\nID_LIKE=arch\n",
            ),
            package_manager: PackageManager::Pacman,
            session: SessionInfo::new(1000, 1000, SessionType::Wayland),
            session_bus: true,
            plasma: KdePresence {
                plasmashell: true,
                kwin: true,
                kglobalaccel: true,
            },
            plasma_version: Some(version("6.8.0")),
            kde_config: KdeConfigPresence {
                kdeglobals: true,
                kwinrc: true,
            },
            data_dir_writable: true,
            architecture: Architecture::X86_64,
            screen_count: Some(1),
        }
    }

    fn status_fixture() -> SystemStatus {
        assemble_status(baseline_inputs())
    }

    #[test]
    fn the_status_dto_carries_every_detected_field() {
        let status = status_fixture();

        assert!(status.supported);
        assert_eq!(status.support.level, SupportLevel::Supported);
        assert!(status.support.reasons.is_empty());
        assert_eq!(status.distro.id.as_deref(), Some("arch"));
        assert_eq!(status.distro.pretty_name.as_deref(), Some("Arch Linux"));
        assert_eq!(status.package_manager, PackageManager::Pacman);
        assert_eq!(status.plasma_version, Some(version("6.8.0")));
        assert!(status.plasmashell_running);
        assert_eq!(status.session_type, SessionType::Wayland);
    }

    #[test]
    fn the_status_dto_serializes_with_camel_case_fields() {
        let value = serde_json::to_value(status_fixture()).expect("serializes");

        assert_eq!(value["supported"], true);
        assert_eq!(value["support"]["level"], "supported");
        assert_eq!(value["support"]["reasons"], serde_json::json!([]));
        assert_eq!(value["distro"]["prettyName"], "Arch Linux");
        assert_eq!(value["distro"]["versionId"], "rolling");
        assert_eq!(value["packageManager"], "pacman");
        assert_eq!(value["plasmaVersion"], "6.8.0");
        assert_eq!(value["plasmashellRunning"], true);
        assert_eq!(value["sessionType"], "wayland");
    }

    #[test]
    fn an_unsupported_status_keeps_the_support_reasons() {
        let mut inputs = baseline_inputs();
        inputs.plasma_version = Some(version("5.27.11"));
        inputs.package_manager = PackageManager::Unknown;

        let status = assemble_status(inputs);
        assert!(!status.supported);
        assert!(status.support.is_refusal());
        assert_eq!(
            status.support.reasons,
            [SupportReason::PlasmaVersionUnsupported]
        );
        assert_eq!(status.package_manager, PackageManager::Unknown);
    }

    #[test]
    fn a_non_linux_status_does_not_require_session_facts() {
        let mut inputs = baseline_inputs();
        inputs.linux = false;
        inputs.session = SessionInfo::new(1, 1, SessionType::Unknown);

        let status = assemble_status(inputs);
        assert!(!status.supported);
        assert_eq!(status.support.reasons, [SupportReason::NotLinux]);
        assert_eq!(status.session_type, SessionType::Unknown);
    }

    #[test]
    fn the_data_dir_probe_writes_and_removes_only_its_own_file() {
        let directory = tempfile::tempdir().expect("tempdir");
        assert!(probe_data_dir_writable(directory.path()));

        let names: Vec<String> = std::fs::read_dir(directory.path())
            .expect("read temp dir")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(names.is_empty(), "leftover files: {names:?}");
    }

    #[test]
    fn a_file_in_place_of_the_data_dir_is_unwritable() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("not-a-directory");
        std::fs::write(&path, b"x").expect("write file");

        assert!(!probe_data_dir_writable(&path));
    }

    #[test]
    fn a_read_only_data_dir_is_unwritable_for_a_normal_user() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o500))
            .expect("make read-only");

        let writable = probe_data_dir_writable(directory.path());

        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
            .expect("restore permissions");

        if crate::platform::session::probe().is_ok_and(|session| session.is_root()) {
            return;
        }
        assert!(!writable);
    }

    #[test]
    fn a_fresh_cache_reuses_the_status_within_its_ttl() {
        let cache = SystemStatusCache::new(Duration::from_secs(60));
        let calls = Cell::new(0);
        let detect = || {
            calls.set(calls.get() + 1);
            Ok(status_fixture())
        };

        cache.get_or_detect(false, detect).expect("first");
        cache.get_or_detect(false, detect).expect("cached");

        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn refresh_bypasses_the_cache() {
        let cache = SystemStatusCache::new(Duration::from_secs(60));
        let calls = Cell::new(0);
        let detect = || {
            calls.set(calls.get() + 1);
            Ok(status_fixture())
        };

        cache.get_or_detect(false, detect).expect("first");
        cache.get_or_detect(true, detect).expect("refresh");

        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_zero_ttl_never_serves_a_stale_status() {
        let cache = SystemStatusCache::new(Duration::ZERO);
        let calls = Cell::new(0);
        let detect = || {
            calls.set(calls.get() + 1);
            Ok(status_fixture())
        };

        cache.get_or_detect(false, detect).expect("first");
        cache.get_or_detect(false, detect).expect("second");

        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_failed_detection_is_not_cached() {
        let cache = SystemStatusCache::new(Duration::from_secs(60));
        let attempts = Cell::new(0);
        let detect = || {
            attempts.set(attempts.get() + 1);
            Err(MorphixError::new(ErrorCode::Internal, "probe failed"))
        };

        assert!(cache.get_or_detect(false, detect).is_err());
        assert!(cache.get_or_detect(false, detect).is_err());

        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn the_cache_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SystemStatusCache>();
    }
}
