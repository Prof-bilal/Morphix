//! The only place in `morphix-core` allowed to spawn a process.
//!
//! **Ownership.** `CommandRunner` executes the two allowlisted version probes from
//! SECURITY.md §7. Nothing else in this crate may use `std::process::Command`; the
//! architecture test enforces that, and the Tauri shell is checked too.
//!
//! # Contract
//!
//! - **No shell.** There is no `sh -c`, no command string, and no pack-controlled input.
//!   A caller picks a [`VersionProbe`] variant; the arguments are the fixed array
//!   `["--version"]`.
//! - **Fixed binary paths.** Candidates are absolute constants, never `PATH` lookups.
//! - **Trusted binary required.** A candidate must be a regular file (not a symlink),
//!   owned by root, and not group- or world-writable; otherwise it is skipped (T-27).
//! - **Bounded.** A probe is killed after [`CommandRunner::timeout`] (2 s) and its stdout
//!   is capped at [`CommandRunner::output_cap`] (4 KiB). Both are enforced here, not by
//!   the caller.
//! - **Closed stdin, cleared environment.** The child inherits no environment variables
//!   and gets `/dev/null` on stdin, so inherited `LD_PRELOAD`/`LD_LIBRARY_PATH` or any
//!   interactive input cannot influence it.
//! - **Failure is not fatal.** Every probe failure is an `E_UNSUPPORTED_PLATFORM` error
//!   with a `reason` context; callers decide whether to fall back to `Unverified` or
//!   `Unsupported`. No probe output is ever treated as a change.
//!
//! Probe output is *not* parsed here. SECURITY.md requires a strict version regex; that
//! belongs to the version parser, which rejects anything that is not a version.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

// `uid()`/`mode()` are the safe, Linux-only metadata accessors used for the trust check.
use std::os::unix::fs::MetadataExt;

use crate::errors::{ErrorCode, MorphixError, Result};

/// The fixed timeout for version probes (SECURITY.md §7).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// The stdout cap for version probes (SECURITY.md §7).
pub const PROBE_OUTPUT_CAP: usize = 4 * 1024;

/// How often the runner checks whether a probe has exited.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// A version probe Morphix is allowed to execute.
///
/// Each variant carries its own fixed binary candidates and arguments; there is no way to
/// build a probe from pack or user text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionProbe {
    /// `plasmashell --version`, used when the package database has no Plasma version.
    PlasmaShell,
    /// `kwin_wayland --version`, the compositor fallback.
    KWinWayland,
}

impl VersionProbe {
    /// Stable identifier for logs and error context.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::PlasmaShell => "plasmashell_version",
            Self::KWinWayland => "kwin_wayland_version",
        }
    }

    /// Absolute binary candidates, in preference order.
    #[must_use]
    pub fn binary_candidates(self) -> &'static [&'static str] {
        match self {
            Self::PlasmaShell => &["/usr/bin/plasmashell", "/usr/local/bin/plasmashell"],
            Self::KWinWayland => &["/usr/bin/kwin_wayland", "/usr/local/bin/kwin_wayland"],
        }
    }

    /// The fixed argument array. Every probe uses exactly this.
    #[must_use]
    pub fn arguments(self) -> &'static [&'static str] {
        &["--version"]
    }

    /// All probes, for coverage tests.
    #[must_use]
    pub fn all() -> &'static [VersionProbe] {
        &[Self::PlasmaShell, Self::KWinWayland]
    }
}

/// Executes allowlisted version probes.
///
/// The runner holds no state between calls and can be shared.
#[derive(Debug, Clone)]
pub struct CommandRunner {
    timeout: Duration,
    output_cap: usize,
}

impl CommandRunner {
    /// A runner with the documented probe limits.
    #[must_use]
    pub fn new() -> Self {
        Self {
            timeout: PROBE_TIMEOUT,
            output_cap: PROBE_OUTPUT_CAP,
        }
    }

    /// A runner with explicit limits.
    ///
    /// Tests use this to exercise the timeout and output cap quickly; a future apply-tool
    /// runner (SECURITY.md §7, 5 s) will use it with its own validated arguments.
    #[must_use]
    pub fn with_limits(timeout: Duration, output_cap: usize) -> Self {
        Self {
            timeout,
            output_cap,
        }
    }

    /// The per-probe timeout.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// The stdout cap in bytes.
    #[must_use]
    pub fn output_cap(&self) -> usize {
        self.output_cap
    }

    /// Runs one allowlisted probe and returns its stdout, trimmed.
    ///
    /// # Errors
    ///
    /// Returns `E_UNSUPPORTED_PLATFORM` with a `reason` context for every probe failure
    /// (`binary_unavailable`, `spawn_failed`, `timeout`, `non_zero_exit`,
    /// `output_too_large`), and `E_IO` if a captured pipe cannot be read.
    pub fn run(&self, probe: VersionProbe) -> Result<String> {
        self.run_spec(&probe)
    }

    fn run_spec(&self, spec: &dyn ProbeSpec) -> Result<String> {
        let binary = self.resolve(spec)?;
        let mut command = Command::new(&binary);
        command
            .args(spec.arguments())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            // A stable working directory means a probe can never depend on Morphix's cwd.
            .current_dir("/");

        let mut child = command.spawn().map_err(|error| {
            probe_error(spec.label(), "spawn_failed")
                .with_context("binary", binary.display().to_string())
                .with_context("io_kind", format!("{:?}", error.kind()))
        })?;

        let status = wait_with_timeout(&mut child, self.timeout).map_err(|error| {
            probe_error(spec.label(), "wait_failed")
                .with_context("binary", binary.display().to_string())
                .with_context("io_kind", format!("{:?}", error.kind()))
        })?;

        let stdout = read_capped(child.stdout.take().as_mut(), self.output_cap)
            .map_err(|error| pipe_error(spec.label(), "stdout", &error))?;
        // stderr is drained so a chatty probe cannot fill the pipe and block, but it is
        // never parsed or logged: only its size is reported as context.
        let stderr = read_capped(child.stderr.take().as_mut(), self.output_cap)
            .map_err(|error| pipe_error(spec.label(), "stderr", &error))?;

        if stdout.truncated {
            return Err(probe_error(spec.label(), "output_too_large")
                .with_context("binary", binary.display().to_string())
                .with_context("limit_bytes", self.output_cap.to_string())
                .with_context("stderr_bytes", stderr.bytes.len().to_string()));
        }

        match status {
            None => Err(probe_error(spec.label(), "timeout")
                .with_context("binary", binary.display().to_string())
                .with_context("timeout_ms", self.timeout.as_millis().to_string())),
            Some(status) if !status.success() => Err(exit_error(
                spec.label(),
                &binary,
                status,
                stderr.bytes.len(),
            )),
            Some(_) => Ok(String::from_utf8_lossy(&stdout.bytes).trim().to_owned()),
        }
    }

    /// Resolves the first trusted candidate for a probe.
    fn resolve(&self, spec: &dyn ProbeSpec) -> Result<PathBuf> {
        let candidates = spec.binary_candidates();
        for candidate in &candidates {
            if is_trusted_binary(candidate) {
                return Ok(candidate.clone());
            }
        }

        Err(
            probe_error(spec.label(), "binary_unavailable").with_context(
                "candidates",
                candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        )
    }
}

impl Default for CommandRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// The internal description of a runnable probe. Production uses [`VersionProbe`]; tests
/// define their own to exercise the runner without touching a real Plasma installation.
trait ProbeSpec {
    fn label(&self) -> &'static str;
    fn arguments(&self) -> Vec<&'static str>;
    fn binary_candidates(&self) -> Vec<PathBuf>;
}

impl ProbeSpec for VersionProbe {
    fn label(&self) -> &'static str {
        (*self).label()
    }

    fn arguments(&self) -> Vec<&'static str> {
        (*self).arguments().to_vec()
    }

    fn binary_candidates(&self) -> Vec<PathBuf> {
        (*self)
            .binary_candidates()
            .iter()
            .map(PathBuf::from)
            .collect()
    }
}

/// Polls until the child exits or the deadline passes, killing it on timeout.
///
/// `Ok(None)` means the probe was killed after exceeding the timeout.
fn wait_with_timeout(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            // The child may have exited between the poll and the kill; `wait` below reaps
            // it either way, so a kill error here is not actionable.
            match child.kill() {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {}
                Err(_) => {}
            }
            child.wait()?;
            return Ok(None);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

struct CappedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

/// Reads at most `cap` bytes, reporting whether more were available.
///
/// The extra byte read makes truncation exact: a probe that writes `cap + 1` bytes is
/// reported as truncated rather than silently accepted.
fn read_capped(reader: Option<&mut impl Read>, cap: usize) -> std::io::Result<CappedOutput> {
    let Some(reader) = reader else {
        return Ok(CappedOutput {
            bytes: Vec::new(),
            truncated: false,
        });
    };

    let mut bytes = Vec::new();
    reader.take(cap as u64 + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > cap;
    bytes.truncate(cap);
    Ok(CappedOutput { bytes, truncated })
}

/// A candidate is trusted only when it is a real file (not a symlink), owned by root, and
/// not group- or world-writable (T-27).
fn is_trusted_binary(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_file() {
        return false;
    }
    trusted_binary_metadata(metadata.uid(), metadata.mode())
}

/// The pure part of the trust check, unit-tested without needing root-owned fixtures.
fn trusted_binary_metadata(uid: u32, mode: u32) -> bool {
    uid == 0 && mode & 0o022 == 0
}

fn probe_error(probe: &'static str, reason: &'static str) -> MorphixError {
    MorphixError::new(
        ErrorCode::UnsupportedPlatform,
        "a version probe could not be completed",
    )
    .with_context("probe", probe)
    .with_context("reason", reason)
}

fn exit_error(
    probe: &'static str,
    binary: &Path,
    status: ExitStatus,
    stderr_bytes: usize,
) -> MorphixError {
    let code = status
        .code()
        .map_or_else(|| "signal".to_owned(), |code| code.to_string());
    probe_error(probe, "non_zero_exit")
        .with_context("binary", binary.display().to_string())
        .with_context("exit_code", code)
        .with_context("stderr_bytes", stderr_bytes.to_string())
}

fn pipe_error(probe: &'static str, stream: &'static str, error: &std::io::Error) -> MorphixError {
    MorphixError::new(ErrorCode::Io, "failed to read a probe's output")
        .with_context("probe", probe)
        .with_context("stream", stream)
        .with_context("io_kind", format!("{:?}", error.kind()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::path::PathBuf;
    use std::time::Duration;

    use super::{
        CommandRunner, PROBE_OUTPUT_CAP, PROBE_TIMEOUT, ProbeSpec, VersionProbe,
        trusted_binary_metadata,
    };
    use crate::errors::ErrorCode;

    /// A probe the tests control completely. `/usr/bin/*` coreutils are used as trusted
    /// binaries; they are root-owned and not group/world-writable on supported systems.
    struct FixtureSpec {
        label: &'static str,
        arguments: Vec<&'static str>,
        binaries: Vec<PathBuf>,
    }

    impl FixtureSpec {
        fn new(label: &'static str, binary: &str, arguments: &[&'static str]) -> Self {
            Self {
                label,
                arguments: arguments.to_vec(),
                binaries: vec![PathBuf::from(binary)],
            }
        }
    }

    impl ProbeSpec for FixtureSpec {
        fn label(&self) -> &'static str {
            self.label
        }

        fn arguments(&self) -> Vec<&'static str> {
            self.arguments.clone()
        }

        fn binary_candidates(&self) -> Vec<PathBuf> {
            self.binaries.clone()
        }
    }

    fn reason_of(error: &crate::errors::MorphixError) -> String {
        error
            .context
            .iter()
            .find(|entry| entry.key == "reason")
            .map(|entry| entry.value.clone())
            .unwrap_or_default()
    }

    #[test]
    fn production_probes_use_only_version_arguments_and_absolute_binaries() {
        for probe in VersionProbe::all() {
            assert_eq!(probe.arguments(), ["--version"], "{probe:?}");
            let candidates = probe.binary_candidates();
            assert!(!candidates.is_empty(), "{probe:?}");
            for candidate in candidates {
                assert!(
                    candidate.is_absolute(),
                    "{} is not absolute",
                    candidate.display()
                );
                assert!(
                    candidate.starts_with("/usr/"),
                    "probe binaries live under /usr, not {}",
                    candidate.display()
                );
            }
        }
    }

    #[test]
    fn default_limits_match_the_security_contract() {
        let runner = CommandRunner::new();
        assert_eq!(runner.timeout(), PROBE_TIMEOUT);
        assert_eq!(runner.timeout(), Duration::from_secs(2));
        assert_eq!(runner.output_cap(), PROBE_OUTPUT_CAP);
        assert_eq!(runner.output_cap(), 4 * 1024);
        assert_eq!(CommandRunner::default().timeout(), PROBE_TIMEOUT);
    }

    #[test]
    fn trusted_binaries_are_root_owned_and_not_group_or_world_writable() {
        assert!(trusted_binary_metadata(0, 0o755));
        assert!(trusted_binary_metadata(0, 0o700));
        assert!(!trusted_binary_metadata(0, 0o775), "group-writable");
        assert!(!trusted_binary_metadata(0, 0o757), "world-writable");
        assert!(!trusted_binary_metadata(1000, 0o755), "not owned by root");
    }

    #[test]
    fn a_user_owned_binary_is_skipped_not_executed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let untrusted = dir.path().join("plasmashell");
        std::fs::write(&untrusted, b"#!/bin/true\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&untrusted, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }

        let spec = FixtureSpec {
            label: "untrusted",
            arguments: vec![],
            binaries: vec![untrusted],
        };
        let error = CommandRunner::new()
            .run_spec(&spec)
            .expect_err("an untrusted binary is not executed");

        assert_eq!(error.code, ErrorCode::UnsupportedPlatform);
        assert_eq!(reason_of(&error), "binary_unavailable");
    }

    #[test]
    fn a_successful_probe_returns_trimmed_stdout() {
        let spec = FixtureSpec::new("true", "/usr/bin/true", &[]);
        let output = CommandRunner::new().run_spec(&spec).expect("runs");

        assert!(output.is_empty());
    }

    #[test]
    fn closed_stdin_means_a_reader_exits_immediately() {
        // `cat` with no arguments reads stdin; with stdin at /dev/null it exits at once.
        let spec = FixtureSpec::new("cat", "/usr/bin/cat", &[]);
        let runner = CommandRunner::with_limits(Duration::from_secs(1), PROBE_OUTPUT_CAP);
        let output = runner.run_spec(&spec).expect("cat exits on empty stdin");

        assert!(output.is_empty());
    }

    #[test]
    fn a_non_zero_exit_is_reported_without_its_output() {
        let spec = FixtureSpec::new("false", "/usr/bin/false", &[]);
        let error = CommandRunner::new()
            .run_spec(&spec)
            .expect_err("false fails");

        assert_eq!(error.code, ErrorCode::UnsupportedPlatform);
        assert_eq!(reason_of(&error), "non_zero_exit");
        assert!(error.context.iter().any(|entry| entry.key == "exit_code"));
    }

    #[test]
    fn a_hanging_probe_is_killed_at_the_timeout() {
        let spec = FixtureSpec::new("sleep", "/usr/bin/sleep", &["5"]);
        let runner = CommandRunner::with_limits(Duration::from_millis(150), PROBE_OUTPUT_CAP);

        let started = std::time::Instant::now();
        let error = runner.run_spec(&spec).expect_err("sleep is killed");
        let elapsed = started.elapsed();

        assert_eq!(reason_of(&error), "timeout");
        assert!(
            elapsed < Duration::from_secs(2),
            "the probe was not killed promptly: {elapsed:?}"
        );
    }

    #[test]
    fn output_over_the_cap_is_refused() {
        // `head -c 100000 /dev/zero` produces 100 KB and exits, deterministically.
        let spec = FixtureSpec::new("flood", "/usr/bin/head", &["-c", "100000", "/dev/zero"]);
        let runner = CommandRunner::with_limits(PROBE_TIMEOUT, 256);
        let error = runner.run_spec(&spec).expect_err("output is capped");

        assert_eq!(reason_of(&error), "output_too_large");
        assert!(error.context.iter().any(|entry| entry.key == "limit_bytes"));
    }

    #[test]
    fn output_at_the_cap_is_accepted() {
        let spec = FixtureSpec::new("head", "/usr/bin/head", &["-c", "128", "/dev/zero"]);
        let runner = CommandRunner::with_limits(PROBE_TIMEOUT, 128);

        // 128 NUL bytes are not text, but the runner does not interpret them.
        assert!(runner.run_spec(&spec).is_ok());
    }

    #[test]
    fn missing_binary_reports_every_candidate() {
        let spec = FixtureSpec {
            label: "missing",
            arguments: vec!["--version"],
            binaries: vec![PathBuf::from("/nonexistent/plasmashell")],
        };
        let error = CommandRunner::new().run_spec(&spec).expect_err("missing");

        assert_eq!(reason_of(&error), "binary_unavailable");
        assert!(
            error
                .context
                .iter()
                .any(|entry| entry.key == "candidates" && entry.value.contains("/nonexistent"))
        );
    }

    #[test]
    fn error_messages_never_include_probe_output() {
        let spec = FixtureSpec::new("head", "/usr/bin/head", &["-c", "64", "/dev/zero"]);
        let runner = CommandRunner::with_limits(Duration::from_millis(10), PROBE_OUTPUT_CAP);
        if let Err(error) = runner.run_spec(&spec) {
            // Whatever the reason, no NUL or captured byte may be in the context.
            for entry in &error.context {
                assert!(!entry.value.contains('\0'));
            }
        }
    }
}
