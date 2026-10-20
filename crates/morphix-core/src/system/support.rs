//! The support gate: whether Morphix may run here, and the closed set of reasons.
//!
//! **Ownership.** Pure rules over already-collected facts. No I/O and no probing; the
//! detector gathers [`SupportInputs`] and calls [`evaluate`]. Keeping this module pure is
//! what lets the unit-test matrix cover every reason without a desktop session.
//!
//! # Verdict semantics
//!
//! - `Unsupported`: a hard gate from MVP.md §1 failed. Mutations must be refused.
//! - `Unverified`: every hard gate passed, but the combination is outside the validated
//!   matrix (MVP.md §1.1). The app remains usable with a warning.
//! - `Supported`: every hard gate passed and the combination matches the validated subset
//!   that Phase 1 can detect (Plasma 6.8.x, Wayland, x86_64). KF/Qt versions are
//!   release-coupled and are recorded by P6-23; a known multi-screen setup is
//!   unverified, while an unknown screen count does not downgrade the verdict.
//!
//! Reasons are machine-readable (`SCREAMING_SNAKE_CASE`) and additive. The summary is
//! presentation text derived from them; the UI must branch on codes, never on the text.

use serde::{Deserialize, Serialize};

use crate::core::versions::PlasmaVersion;
use crate::desktop::kde::{KdeConfigPresence, KdePresence};
use crate::platform::session::{SessionInfo, SessionType};

/// The support verdict shown before any pack action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    /// All hard gates pass and the combination is in the validated subset.
    Supported,
    /// All hard gates pass, but the combination has not been validated.
    Unverified,
    /// A hard gate failed; mutations must be refused.
    Unsupported,
}

impl SupportLevel {
    /// Stable lowercase identifier, also used in IPC.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Unverified => "unverified",
            Self::Unsupported => "unsupported",
        }
    }
}

/// A machine-readable reason for a non-supported verdict.
///
/// The serde names are the IPC contract. Hard-gate reasons imply
/// [`SupportLevel::Unsupported`]; validation reasons imply [`SupportLevel::Unverified`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SupportReason {
    /// `/proc` or `/etc/os-release` is unavailable.
    #[serde(rename = "NOT_LINUX")]
    NotLinux,
    /// The process is effectively root.
    #[serde(rename = "RUNNING_AS_ROOT")]
    RunningAsRoot,
    /// The user session bus could not be reached.
    #[serde(rename = "NO_SESSION_BUS")]
    NoSessionBus,
    /// `org.kde.plasmashell` has no owner.
    #[serde(rename = "NO_PLASMA")]
    NoPlasma,
    /// A Plasma version was found but it is outside `>=6.0, <7.0`.
    #[serde(rename = "PLASMA_VERSION_UNSUPPORTED")]
    PlasmaVersionUnsupported,
    /// Plasma is running but its version could not be determined.
    #[serde(rename = "PLASMA_VERSION_UNKNOWN")]
    PlasmaVersionUnknown,
    /// `org.kde.KWin` has no owner.
    #[serde(rename = "KWIN_UNAVAILABLE")]
    KWinUnavailable,
    /// `org.kde.kglobalaccel` has no owner.
    #[serde(rename = "KGLOBALACCEL_UNAVAILABLE")]
    KGlobalAccelUnavailable,
    /// `kdeglobals` is missing or unreadable.
    #[serde(rename = "KDEGLOBALS_UNREADABLE")]
    KdeglobalsUnreadable,
    /// `kwinrc` is missing or unreadable.
    #[serde(rename = "KWINRC_UNREADABLE")]
    KwinrcUnreadable,
    /// The Morphix data directory could not be written.
    #[serde(rename = "DATA_DIR_UNWRITABLE")]
    DataDirUnwritable,
    /// The Plasma minor is outside the validated 6.8 line, or is a pre-release.
    #[serde(rename = "PLASMA_MINOR_UNVALIDATED")]
    PlasmaMinorUnvalidated,
    /// The session is not Wayland (X11 and unknown are unverified).
    #[serde(rename = "SESSION_TYPE_UNVALIDATED")]
    SessionTypeUnvalidated,
    /// The architecture is not x86_64.
    #[serde(rename = "ARCHITECTURE_UNVALIDATED")]
    ArchitectureUnvalidated,
    /// More than one screen is known to be present.
    #[serde(rename = "MULTI_SCREEN_UNVALIDATED")]
    MultiScreenUnvalidated,
}

impl SupportReason {
    /// Every reason, in presentation order. The matrix test iterates this list so a new
    /// variant cannot be added without a reachable input case.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::NotLinux,
            Self::RunningAsRoot,
            Self::NoSessionBus,
            Self::NoPlasma,
            Self::PlasmaVersionUnsupported,
            Self::PlasmaVersionUnknown,
            Self::KWinUnavailable,
            Self::KGlobalAccelUnavailable,
            Self::KdeglobalsUnreadable,
            Self::KwinrcUnreadable,
            Self::DataDirUnwritable,
            Self::PlasmaMinorUnvalidated,
            Self::SessionTypeUnvalidated,
            Self::ArchitectureUnvalidated,
            Self::MultiScreenUnvalidated,
        ]
    }

    /// Whether this reason is a hard refusal rather than an unverified warning.
    #[must_use]
    pub fn is_refusal(self) -> bool {
        matches!(
            self,
            Self::NotLinux
                | Self::RunningAsRoot
                | Self::NoSessionBus
                | Self::NoPlasma
                | Self::PlasmaVersionUnsupported
                | Self::PlasmaVersionUnknown
                | Self::KWinUnavailable
                | Self::KGlobalAccelUnavailable
                | Self::KdeglobalsUnreadable
                | Self::KwinrcUnreadable
                | Self::DataDirUnwritable
        )
    }

    /// The verdict level this reason belongs to.
    #[must_use]
    pub fn level(self) -> SupportLevel {
        if self.is_refusal() {
            SupportLevel::Unsupported
        } else {
            SupportLevel::Unverified
        }
    }

    /// Human-readable explanation used only to build the summary.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::NotLinux => "this is not a Linux system",
            Self::RunningAsRoot => "Morphix is running as root",
            Self::NoSessionBus => "the Plasma session bus is not reachable",
            Self::NoPlasma => "Plasma is not running in this session",
            Self::PlasmaVersionUnsupported => {
                "the Plasma version is outside the supported 6.x range"
            }
            Self::PlasmaVersionUnknown => "the Plasma version could not be determined",
            Self::KWinUnavailable => "the KWin compositor is not reachable",
            Self::KGlobalAccelUnavailable => "the global shortcut service is not reachable",
            Self::KdeglobalsUnreadable => "the KDE configuration file is not readable",
            Self::KwinrcUnreadable => "the KWin configuration file is not readable",
            Self::DataDirUnwritable => "the Morphix data directory is not writable",
            Self::PlasmaMinorUnvalidated => "this Plasma minor has not been validated",
            Self::SessionTypeUnvalidated => "this session type has not been validated",
            Self::ArchitectureUnvalidated => "this architecture has not been validated",
            Self::MultiScreenUnvalidated => "multi-screen setups have not been validated",
        }
    }
}

/// The CPU architecture class used by the validated matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Architecture {
    /// x86_64, the primary validated architecture.
    #[serde(rename = "x86_64")]
    X86_64,
    /// 64-bit ARM, not validated in V0.1.
    #[serde(rename = "aarch64")]
    Aarch64,
    /// Any other architecture.
    #[serde(rename = "other")]
    Other,
}

impl Architecture {
    /// Classifies the compiled target architecture.
    ///
    /// This is the architecture Morphix was built for, not an uname probe; a 32-bit user
    /// space on a 64-bit kernel is correctly reported as non-x86_64.
    #[must_use]
    pub fn current() -> Self {
        match std::env::consts::ARCH {
            "x86_64" => Self::X86_64,
            "aarch64" => Self::Aarch64,
            _ => Self::Other,
        }
    }
}

/// Facts the detector collects before evaluating the gate.
///
/// Booleans are deliberate: the rules do not need to know why a probe failed, only that it
/// did. `linux` is true only when `/proc` exists and `/etc/os-release` is readable.
#[derive(Debug, Clone)]
pub struct SupportInputs {
    /// Linux markers are present.
    pub linux: bool,
    /// The session bus accepted a connection.
    pub session_bus: bool,
    /// KDE service presence from the session bus.
    pub plasma: KdePresence,
    /// Detected Plasma version, if any source produced one.
    pub plasma_version: Option<PlasmaVersion>,
    /// Readability of the required KDE config files.
    pub kde_config: KdeConfigPresence,
    /// Process UID and session type.
    pub session: SessionInfo,
    /// The data directory passed the write probe.
    pub data_dir_writable: bool,
    /// Compiled target architecture.
    pub architecture: Architecture,
    /// Screen count when known; `None` means not probed, `Some(0)` is treated as unknown.
    pub screen_count: Option<u32>,
}

/// The verdict and its machine-readable reasons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportReport {
    /// `supported`, `unverified`, or `unsupported`.
    pub level: SupportLevel,
    /// Empty only for a supported verdict.
    pub reasons: Vec<SupportReason>,
    /// Human-readable explanation. The UI must not branch on this text.
    pub summary: String,
}

impl SupportReport {
    /// Whether the system passed every hard gate and is validated.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.level == SupportLevel::Supported
    }

    /// Whether a hard gate failed and mutations must be refused.
    #[must_use]
    pub fn is_refusal(&self) -> bool {
        self.level == SupportLevel::Unsupported
    }

    fn new(level: SupportLevel, reasons: Vec<SupportReason>) -> Self {
        let summary = summarize(level, &reasons);
        Self {
            level,
            reasons,
            summary,
        }
    }
}

/// Evaluates the support gate for already-collected facts.
///
/// Checks are ordered root-cause first: a missing session bus suppresses the DBus and
/// config reasons that would otherwise be noise, and a missing plasmashell suppresses the
/// interface and version reasons. Independent failures (root, data directory) are still
/// reported together.
#[must_use]
pub fn evaluate(inputs: &SupportInputs) -> SupportReport {
    if !inputs.linux {
        return SupportReport::new(SupportLevel::Unsupported, vec![SupportReason::NotLinux]);
    }

    let mut reasons = Vec::new();

    if inputs.session.is_root() {
        reasons.push(SupportReason::RunningAsRoot);
    }

    if !inputs.session_bus {
        reasons.push(SupportReason::NoSessionBus);
    } else if !inputs.plasma.plasmashell {
        reasons.push(SupportReason::NoPlasma);
    } else {
        match inputs.plasma_version.as_ref() {
            Some(version) if !version.is_plasma_6() => {
                reasons.push(SupportReason::PlasmaVersionUnsupported);
            }
            Some(_) => {}
            None => reasons.push(SupportReason::PlasmaVersionUnknown),
        }

        if !inputs.plasma.kwin {
            reasons.push(SupportReason::KWinUnavailable);
        }
        if !inputs.plasma.kglobalaccel {
            reasons.push(SupportReason::KGlobalAccelUnavailable);
        }
        if !inputs.kde_config.kdeglobals {
            reasons.push(SupportReason::KdeglobalsUnreadable);
        }
        if !inputs.kde_config.kwinrc {
            reasons.push(SupportReason::KwinrcUnreadable);
        }
    }

    if !inputs.data_dir_writable {
        reasons.push(SupportReason::DataDirUnwritable);
    }

    if !reasons.is_empty() {
        return SupportReport::new(SupportLevel::Unsupported, reasons);
    }

    let mut warnings = Vec::new();

    if let Some(version) = inputs.plasma_version.as_ref() {
        let release_is_validated =
            version.major() == 6 && version.minor() == 8 && version.as_semver().pre.is_empty();
        if !release_is_validated {
            warnings.push(SupportReason::PlasmaMinorUnvalidated);
        }
    }

    if inputs.session.session_type != SessionType::Wayland {
        warnings.push(SupportReason::SessionTypeUnvalidated);
    }
    if inputs.architecture != Architecture::X86_64 {
        warnings.push(SupportReason::ArchitectureUnvalidated);
    }
    if inputs.screen_count.is_some_and(|count| count > 1) {
        warnings.push(SupportReason::MultiScreenUnvalidated);
    }

    if warnings.is_empty() {
        SupportReport::new(SupportLevel::Supported, Vec::new())
    } else {
        SupportReport::new(SupportLevel::Unverified, warnings)
    }
}

fn summarize(level: SupportLevel, reasons: &[SupportReason]) -> String {
    let labels = reasons
        .iter()
        .map(|reason| reason.label())
        .collect::<Vec<_>>()
        .join("; ");

    match level {
        SupportLevel::Supported => "This system meets Morphix's support requirements.".to_owned(),
        SupportLevel::Unsupported => {
            format!("Morphix cannot change settings on this system: {labels}.")
        }
        SupportLevel::Unverified => format!(
            "Morphix has not validated this system combination: {labels}. \
             You can continue, but some features may behave differently."
        ),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::collections::BTreeSet;

    use super::{
        Architecture, SupportInputs, SupportLevel, SupportReason, SupportReport, evaluate,
    };
    use crate::core::versions::PlasmaVersion;
    use crate::desktop::kde::{KdeConfigPresence, KdePresence};
    use crate::platform::session::{SessionInfo, SessionType};

    fn version(text: &str) -> PlasmaVersion {
        PlasmaVersion::parse(text).expect("valid test version")
    }

    fn baseline() -> SupportInputs {
        SupportInputs {
            linux: true,
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
            session: SessionInfo::new(1000, 1000, SessionType::Wayland),
            data_dir_writable: true,
            architecture: Architecture::X86_64,
            screen_count: Some(1),
        }
    }

    fn inputs_for(reason: SupportReason) -> SupportInputs {
        let mut inputs = baseline();
        match reason {
            SupportReason::NotLinux => inputs.linux = false,
            SupportReason::RunningAsRoot => {
                inputs.session = SessionInfo::new(0, 0, SessionType::Wayland);
            }
            SupportReason::NoSessionBus => inputs.session_bus = false,
            SupportReason::NoPlasma => inputs.plasma.plasmashell = false,
            SupportReason::PlasmaVersionUnsupported => {
                inputs.plasma_version = Some(version("5.27.11"));
            }
            SupportReason::PlasmaVersionUnknown => inputs.plasma_version = None,
            SupportReason::KWinUnavailable => inputs.plasma.kwin = false,
            SupportReason::KGlobalAccelUnavailable => inputs.plasma.kglobalaccel = false,
            SupportReason::KdeglobalsUnreadable => inputs.kde_config.kdeglobals = false,
            SupportReason::KwinrcUnreadable => inputs.kde_config.kwinrc = false,
            SupportReason::DataDirUnwritable => inputs.data_dir_writable = false,
            SupportReason::PlasmaMinorUnvalidated => {
                inputs.plasma_version = Some(version("6.7.0"));
            }
            SupportReason::SessionTypeUnvalidated => {
                inputs.session = SessionInfo::new(1000, 1000, SessionType::X11);
            }
            SupportReason::ArchitectureUnvalidated => inputs.architecture = Architecture::Aarch64,
            SupportReason::MultiScreenUnvalidated => inputs.screen_count = Some(2),
        }
        inputs
    }

    fn summary(report: &SupportReport) -> &str {
        report.summary.as_str()
    }

    #[test]
    fn the_baseline_is_supported_without_reasons() {
        let report = evaluate(&baseline());

        assert_eq!(report.level, SupportLevel::Supported);
        assert!(report.reasons.is_empty());
        assert!(report.is_supported());
        assert!(!report.is_refusal());
        assert!(summary(&report).contains("support requirements"));
    }

    #[test]
    fn every_reason_is_reachable_and_has_the_right_level() {
        for &reason in SupportReason::all() {
            let report = evaluate(&inputs_for(reason));

            assert_eq!(
                report.level,
                reason.level(),
                "{reason:?} produced {report:?}"
            );
            assert!(
                report.reasons.contains(&reason),
                "{reason:?} missing from {:?}",
                report.reasons
            );
            assert!(!summary(&report).is_empty(), "{reason:?} has no summary");
        }
    }

    #[test]
    fn every_reason_has_a_distinct_wire_name() {
        let names: BTreeSet<String> = SupportReason::all()
            .iter()
            .map(|reason| {
                serde_json::to_value(reason)
                    .expect("serializes")
                    .as_str()
                    .expect("reason is a string")
                    .to_owned()
            })
            .collect();

        assert_eq!(names.len(), SupportReason::all().len());
        assert!(names.contains("NOT_LINUX"));
        assert!(names.contains("NO_SESSION_BUS"));
        assert!(names.contains("KWIN_UNAVAILABLE"));
        assert!(names.contains("KGLOBALACCEL_UNAVAILABLE"));
        assert!(names.contains("MULTI_SCREEN_UNVALIDATED"));
    }

    #[test]
    fn a_missing_session_bus_suppresses_dbus_dependent_reasons() {
        let mut inputs = baseline();
        inputs.session_bus = false;
        inputs.plasma = KdePresence::default();
        inputs.plasma_version = None;
        inputs.kde_config = KdeConfigPresence::default();

        let report = evaluate(&inputs);
        assert_eq!(report.level, SupportLevel::Unsupported);
        assert_eq!(report.reasons, [SupportReason::NoSessionBus]);
    }

    #[test]
    fn a_missing_plasmashell_suppresses_interface_and_version_reasons() {
        let mut inputs = baseline();
        inputs.plasma.plasmashell = false;
        inputs.plasma.kwin = false;
        inputs.plasma.kglobalaccel = false;
        inputs.plasma_version = None;
        inputs.kde_config = KdeConfigPresence::default();

        let report = evaluate(&inputs);
        assert_eq!(report.level, SupportLevel::Unsupported);
        assert_eq!(report.reasons, [SupportReason::NoPlasma]);
    }

    #[test]
    fn independent_refusals_are_reported_together() {
        let mut inputs = baseline();
        inputs.session = SessionInfo::new(0, 0, SessionType::Wayland);
        inputs.data_dir_writable = false;

        let report = evaluate(&inputs);
        assert_eq!(
            report.reasons,
            [
                SupportReason::RunningAsRoot,
                SupportReason::DataDirUnwritable
            ]
        );
    }

    #[test]
    fn plasma_5_is_refused() {
        let mut inputs = baseline();
        inputs.plasma_version = Some(version("5.27.11"));

        let report = evaluate(&inputs);
        assert!(report.is_refusal());
        assert_eq!(report.reasons, [SupportReason::PlasmaVersionUnsupported]);
    }

    #[test]
    fn an_unknown_plasma_version_is_not_guessed() {
        let mut inputs = baseline();
        inputs.plasma_version = None;

        let report = evaluate(&inputs);
        assert!(report.is_refusal());
        assert_eq!(report.reasons, [SupportReason::PlasmaVersionUnknown]);
    }

    #[test]
    fn an_unvalidated_combination_reports_every_warning() {
        let mut inputs = baseline();
        inputs.plasma_version = Some(version("6.7.0"));
        inputs.session = SessionInfo::new(1000, 1000, SessionType::X11);
        inputs.architecture = Architecture::Other;
        inputs.screen_count = Some(2);

        let report = evaluate(&inputs);
        assert_eq!(report.level, SupportLevel::Unverified);
        assert!(!report.is_refusal());
        assert_eq!(
            report.reasons,
            [
                SupportReason::PlasmaMinorUnvalidated,
                SupportReason::SessionTypeUnvalidated,
                SupportReason::ArchitectureUnvalidated,
                SupportReason::MultiScreenUnvalidated,
            ]
        );
        assert!(summary(&report).contains("not validated"));
    }

    #[test]
    fn later_68_patch_releases_and_unknown_screen_counts_stay_validated() {
        let mut inputs = baseline();
        inputs.plasma_version = Some(version("6.8.1"));
        inputs.screen_count = None;
        assert_eq!(evaluate(&inputs).level, SupportLevel::Supported);

        inputs.plasma_version = Some(version("6.9.0"));
        assert_eq!(
            evaluate(&inputs).reasons,
            [SupportReason::PlasmaMinorUnvalidated]
        );
    }

    #[test]
    fn a_68_pre_release_is_unverified() {
        let mut inputs = baseline();
        inputs.plasma_version = Some(version("6.8.0-rc1"));

        let report = evaluate(&inputs);
        assert_eq!(report.level, SupportLevel::Unverified);
        assert_eq!(report.reasons, [SupportReason::PlasmaMinorUnvalidated]);
    }

    #[test]
    fn a_non_linux_failure_is_terminal() {
        let mut inputs = baseline();
        inputs.linux = false;
        inputs.session = SessionInfo::new(0, 0, SessionType::Unknown);
        inputs.data_dir_writable = false;

        let report = evaluate(&inputs);
        assert_eq!(report.reasons, [SupportReason::NotLinux]);
    }

    #[test]
    fn the_report_serializes_for_ipc() {
        let report = evaluate(&inputs_for(SupportReason::RunningAsRoot));
        let value = serde_json::to_value(&report).expect("serializes");

        assert_eq!(value["level"], "unsupported");
        assert_eq!(value["reasons"], serde_json::json!(["RUNNING_AS_ROOT"]));
        assert!(
            value["summary"]
                .as_str()
                .is_some_and(|text| text.contains("root"))
        );
    }

    #[test]
    fn every_level_has_the_documented_wire_name() {
        assert_eq!(
            serde_json::to_value(SupportLevel::Supported).expect("serializes"),
            serde_json::json!("supported")
        );
        assert_eq!(
            serde_json::to_value(SupportLevel::Unverified).expect("serializes"),
            serde_json::json!("unverified")
        );
        assert_eq!(
            serde_json::to_value(SupportLevel::Unsupported).expect("serializes"),
            serde_json::json!("unsupported")
        );
        assert_eq!(SupportLevel::Supported.as_str(), "supported");
    }

    #[test]
    fn the_architecture_class_matches_the_compiled_target() {
        match std::env::consts::ARCH {
            "x86_64" => assert_eq!(Architecture::current(), Architecture::X86_64),
            "aarch64" => assert_eq!(Architecture::current(), Architecture::Aarch64),
            _ => assert_eq!(Architecture::current(), Architecture::Other),
        }
    }
}
