//! Session facts: user identity and display session type.
//!
//! **Ownership.** Read-only probing of the current process and environment. It does not
//! contact DBus (that belongs to `desktop::kde`), read KDE config, or mutate anything.
//!
//! # Root policy
//!
//! Morphix refuses to mutate when the effective UID is 0 (SECURITY.md SO-5, T-07;
//! AC-22). Detection still *runs* as root so the UI can explain the refusal; every
//! mutation path must call [`SessionInfo::require_non_root`], which returns
//! `E_RUNNING_AS_ROOT`. Using the effective UID, not the real UID, matches
//! `geteuid()` in MVP.md §1.
//!
//! # Why `/proc/self/status`
//!
//! `std` has no safe API for the effective UID, and `morphix-core` forbids `unsafe`, so
//! the UID fields are read from `/proc/self/status`. This is Linux-only, which is already
//! the product boundary (`/proc` presence is part of the support gate), and it gives the
//! real, effective, saved, and filesystem UIDs in one place. A malformed or unavailable
//! file is an error rather than an assumed non-root result: failing open on a security
//! check would be wrong.
//!
//! # Session type
//!
//! `XDG_SESSION_TYPE` is the documented source (`wayland`, `x11`, `tty`). An unset,
//! empty, or unrecognized value becomes [`SessionType::Unknown`]; we do not guess from
//! `DISPLAY` or `WAYLAND_DISPLAY`, because the docs make those variables informational
//! and a wrong guess would be worse than an honest unknown.

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, MorphixError, Result};

/// Path read for the process UIDs. Linux exposes all four UID fields here.
pub const PROC_SELF_STATUS: &str = "/proc/self/status";

/// Environment variable that names the display session.
pub const SESSION_TYPE_ENV: &str = "XDG_SESSION_TYPE";

/// The display session in which Morphix is running.
///
/// This is informational only: both Wayland and X11 are accepted, with X11 currently
/// unverified for some KDE paths (PRD.md §"Validated matrix").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    /// A Wayland compositor session.
    Wayland,
    /// An X11 session.
    X11,
    /// A text console, not a graphical desktop.
    Tty,
    /// Missing or unrecognized `XDG_SESSION_TYPE`.
    #[default]
    Unknown,
}

impl SessionType {
    /// Stable lowercase identifier, also used in IPC.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::X11 => "x11",
            Self::Tty => "tty",
            Self::Unknown => "unknown",
        }
    }

    /// Human-readable label for the status card.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Wayland => "Wayland",
            Self::X11 => "X11",
            Self::Tty => "TTY",
            Self::Unknown => "Unknown",
        }
    }

    /// Classifies a raw `XDG_SESSION_TYPE` value. Unknown values are not errors: the
    /// variable is informational (MVP.md §1).
    #[must_use]
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("wayland") => Self::Wayland,
            Some("x11") => Self::X11,
            Some("tty") => Self::Tty,
            _ => Self::Unknown,
        }
    }
}

/// The current process's identity and session type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionInfo {
    /// Real UID.
    pub uid: u32,
    /// Effective UID. Root is decided from this field.
    pub effective_uid: u32,
    /// Display session, when known.
    pub session_type: SessionType,
}

impl SessionInfo {
    /// Builds a session value from already-known parts.
    #[must_use]
    pub fn new(uid: u32, effective_uid: u32, session_type: SessionType) -> Self {
        Self {
            uid,
            effective_uid,
            session_type,
        }
    }

    /// Whether the process is effectively root.
    #[must_use]
    pub fn is_root(self) -> bool {
        self.effective_uid == 0
    }

    /// Refuses to continue when running as root.
    ///
    /// # Errors
    ///
    /// Returns `E_RUNNING_AS_ROOT` when [`SessionInfo::is_root`] is true. The message
    /// states that nothing was changed, because this check runs before any mutation.
    pub fn require_non_root(self) -> Result<()> {
        if self.is_root() {
            Err(MorphixError::new(
                ErrorCode::RunningAsRoot,
                "Morphix refuses to change settings while running as root",
            )
            .with_context("effective_uid", self.effective_uid.to_string()))
        } else {
            Ok(())
        }
    }
}

/// Probes the current process and environment.
///
/// # Errors
///
/// Returns `E_IO` when `/proc/self/status` cannot be read, and `E_UNSUPPORTED_PLATFORM`
/// when its UID fields are missing or malformed. The check never assumes non-root.
pub fn probe() -> Result<SessionInfo> {
    let status = std::fs::read_to_string(PROC_SELF_STATUS).map_err(|error| {
        MorphixError::new(ErrorCode::Io, "failed to read the process status")
            .with_context("path", PROC_SELF_STATUS)
            .with_context("io_kind", format!("{:?}", error.kind()))
    })?;
    let (uid, effective_uid) = parse_uids(&status)?;
    let session_type = SessionType::from_env_value(std::env::var(SESSION_TYPE_ENV).ok().as_deref());

    Ok(SessionInfo::new(uid, effective_uid, session_type))
}

/// Parses the real and effective UID from `/proc/self/status` content.
fn parse_uids(status: &str) -> Result<(u32, u32)> {
    let line = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .ok_or_else(|| missing_uid_error("Uid: line is missing"))?;

    let mut fields = line.split_ascii_whitespace().skip(1);
    let uid = fields
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| missing_uid_error("real UID is missing or not a number"))?;
    let effective_uid = fields
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| missing_uid_error("effective UID is missing or not a number"))?;

    Ok((uid, effective_uid))
}

fn missing_uid_error(reason: &'static str) -> MorphixError {
    MorphixError::new(
        ErrorCode::UnsupportedPlatform,
        "cannot determine the effective user; Morphix will not assume it is unprivileged",
    )
    .with_context("path", PROC_SELF_STATUS)
    .with_context("reason", reason)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{SessionInfo, SessionType, parse_uids};
    use crate::errors::ErrorCode;

    const NORMAL_STATUS: &str =
        "Name:\tmorphix\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\n";
    const ROOT_STATUS: &str = "Name:\tmorphix\nUid:\t0\t0\t0\t0\n";

    #[test]
    fn classifies_the_documented_session_types() {
        assert_eq!(
            SessionType::from_env_value(Some("wayland")),
            SessionType::Wayland
        );
        assert_eq!(SessionType::from_env_value(Some("x11")), SessionType::X11);
        assert_eq!(SessionType::from_env_value(Some("tty")), SessionType::Tty);
    }

    #[test]
    fn unknown_or_missing_session_values_are_not_guessed() {
        for value in [None, Some(""), Some("  "), Some("mir"), Some("XWAYLAND")] {
            assert_eq!(
                SessionType::from_env_value(value),
                SessionType::Unknown,
                "value: {value:?}"
            );
        }
    }

    #[test]
    fn session_type_matching_is_case_insensitive_and_trimmed() {
        assert_eq!(
            SessionType::from_env_value(Some("  WaYlAnD\n")),
            SessionType::Wayland
        );
        assert_eq!(SessionType::from_env_value(Some(" X11 ")), SessionType::X11);
    }

    #[test]
    fn parses_real_and_effective_uid() {
        assert_eq!(
            parse_uids(NORMAL_STATUS).expect("normal status"),
            (1000, 1000)
        );
        assert_eq!(parse_uids(ROOT_STATUS).expect("root status"), (0, 0));
    }

    #[test]
    fn root_is_decided_from_the_effective_uid() {
        let effective_root = SessionInfo::new(1000, 0, SessionType::Wayland);
        assert!(effective_root.is_root());
        assert_eq!(
            effective_root
                .require_non_root()
                .expect_err("root is refused")
                .code,
            ErrorCode::RunningAsRoot
        );

        let real_root_only = SessionInfo::new(0, 1000, SessionType::Wayland);
        assert!(!real_root_only.is_root());
        assert!(real_root_only.require_non_root().is_ok());
    }

    #[test]
    fn non_root_sessions_are_accepted() {
        let info = SessionInfo::new(1000, 1000, SessionType::X11);
        assert!(!info.is_root());
        assert!(info.require_non_root().is_ok());
    }

    #[test]
    fn malformed_status_never_defaults_to_non_root() {
        for status in [
            "",
            "Name:\tmorphix\n",
            "Uid:\n",
            "Uid:\tnotanumber\t1000\n",
            "Uid:\t1000\n",
            "uid:\t1000\t1000\n",
        ] {
            let error = parse_uids(status).expect_err("malformed status is refused");
            assert_eq!(
                error.code,
                ErrorCode::UnsupportedPlatform,
                "status: {status:?}"
            );
            assert!(error.context.iter().any(|entry| entry.key == "reason"));
        }
    }

    #[test]
    fn additional_status_fields_and_extra_whitespace_are_tolerated() {
        let status = "Name:\tmorphix\nUid:   1000   1000   1000   1000\nGroups:\t1000\n";
        assert_eq!(parse_uids(status).expect("valid status"), (1000, 1000));
    }

    #[test]
    fn identifiers_and_labels_are_stable() {
        assert_eq!(SessionType::Wayland.as_str(), "wayland");
        assert_eq!(SessionType::Wayland.label(), "Wayland");
        assert_eq!(SessionType::X11.label(), "X11");
        assert_eq!(SessionType::Tty.label(), "TTY");
        assert_eq!(SessionType::Unknown.label(), "Unknown");
    }

    #[test]
    fn serde_uses_the_documented_names() {
        for (session, expected) in [
            (SessionType::Wayland, "\"wayland\""),
            (SessionType::X11, "\"x11\""),
            (SessionType::Tty, "\"tty\""),
            (SessionType::Unknown, "\"unknown\""),
        ] {
            assert_eq!(
                serde_json::to_string(&session).expect("serializes"),
                expected
            );
        }
    }
}
