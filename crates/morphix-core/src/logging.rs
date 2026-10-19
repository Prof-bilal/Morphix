//! Logging bootstrap: subscriber setup, log path and rotation, file mode, redaction.
//!
//! **Ownership.** The only module that configures a log destination, and the place where
//! the rule "no file contents, no secrets, structured fields only" is enforced
//! (SECURITY.md §13, AGENTS.md §10).
//!
//! **Allowed dependencies:** `errors` (plus the logging crates).
//!
//! # What is written
//!
//! Logs live at `$XDG_STATE_HOME/morphix/logs/morphix.log` (or the `MORPHIX_STATE_DIR`
//! override), are created with mode `0600`, and rotate by size. Allowed fields are the ones
//! in SECURITY.md §13: timestamp, level, event, transaction id, plan id, pack id, target
//! path, config key, error code.
//!
//! # What is never written
//!
//! The values of fields named in [`REDACTED_FIELDS`] are replaced with
//! [`REDACTION_PLACEHOLDER`] before the line reaches the file, so a caller that
//! accidentally logs a config value or a file body cannot leak it. Redaction is by field
//! name: a message that embeds untrusted content is a caller bug, not something this module
//! can detect.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::{FormatFields, Writer};

use crate::errors::{ErrorCode, MorphixError};

/// File name of the active log inside `<state dir>/logs/`.
pub const LOG_FILE_NAME: &str = "morphix.log";

/// Directory holding the log file, relative to the XDG state directory.
pub const LOG_DIR_NAME: &str = "logs";

/// Field names whose values are never written (SECURITY.md §13, threat T-23).
///
/// Matching is ASCII case-insensitive. Names are chosen to match how callers naturally
/// name the data, so a careless `tracing::info!` still cannot log a file body or a secret.
pub const REDACTED_FIELDS: &[&str] = &[
    "asset_bytes",
    "body",
    "config_value",
    "content",
    "contents",
    "file_content",
    "file_contents",
    "password",
    "secret",
    "token",
];

/// Replacement written instead of a redacted value.
pub const REDACTION_PLACEHOLDER: &str = "[redacted]";

/// Returns the log file path for a state directory.
#[must_use]
pub fn log_path(state_dir: &Path) -> PathBuf {
    state_dir.join(LOG_DIR_NAME).join(LOG_FILE_NAME)
}

/// Returns whether a field name is on the redaction list.
#[must_use]
pub fn is_redacted_field(name: &str) -> bool {
    REDACTED_FIELDS
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

/// Startup options for the logging subsystem.
#[derive(Debug, Clone)]
pub struct LogOptions {
    /// Root state directory; the log goes in its `logs/` subdirectory.
    pub state_dir: PathBuf,
    /// Maximum level to record.
    pub level: tracing::Level,
    /// Rotate once the active file would exceed this size. `0` disables rotation.
    pub max_bytes: u64,
    /// Number of files to keep, including the active one. At least 1.
    pub max_files: usize,
}

impl LogOptions {
    /// Log options for a state directory with the documented defaults: `info`, 1 MiB per
    /// file, five files kept.
    #[must_use]
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
            level: tracing::Level::INFO,
            max_bytes: 1024 * 1024,
            max_files: 5,
        }
    }

    /// The active log file path these options resolve to.
    #[must_use]
    pub fn file_path(&self) -> PathBuf {
        log_path(&self.state_dir)
    }
}

/// Handle to an initialised logging subsystem.
#[derive(Debug, Clone)]
pub struct LogHandle {
    /// The active log file.
    pub path: PathBuf,
}

/// Installs the global subscriber.
///
/// # Errors
///
/// Fails with `E_IO` if the log directory or file cannot be created with mode `0600`, and
/// with `E_INTERNAL` if a global subscriber was already installed. Both cases carry the
/// path or cause as context; neither is silently ignored.
pub fn init(options: &LogOptions) -> Result<LogHandle, MorphixError> {
    let path = options.file_path();
    let writer =
        RotatingLog::open(&path, options.max_bytes, options.max_files).map_err(|error| {
            MorphixError::new(ErrorCode::Io, "could not open the Morphix log file")
                .with_context("path", path.display().to_string())
                .with_context("cause", error.to_string())
        })?;

    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_max_level(options.level)
        // A log file is read with `less`; ANSI escapes are noise, not formatting.
        .with_ansi(false)
        .fmt_fields(RedactingFields)
        .finish();

    tracing::subscriber::set_global_default(subscriber).map_err(|error| {
        MorphixError::new(ErrorCode::Internal, "logging was already initialised")
            .with_context("cause", error.to_string())
    })?;

    Ok(LogHandle { path })
}

/// A size-rotating, mode-`0600` log file that can be cloned into every formatting thread.
#[derive(Debug, Clone)]
pub struct RotatingLog {
    state: Arc<Mutex<FileState>>,
}

#[derive(Debug)]
struct FileState {
    file: File,
    path: PathBuf,
    written: u64,
    max_bytes: u64,
    max_files: usize,
    /// Bytes seen since the last newline. A record is only rotated at a line boundary, so
    /// a record is never split across two files.
    pending: Vec<u8>,
}

/// Flush a pending partial line if it grows past this, so one pathological log line cannot
/// hold unbounded memory.
const MAX_PENDING_BYTES: usize = 64 * 1024;

impl RotatingLog {
    /// Opens (creating if needed) the log file, applying the rotation policy.
    ///
    /// # Errors
    ///
    /// Returns the underlying `std::io::Error` for a failed directory or file creation.
    pub fn open(path: &Path, max_bytes: u64, max_files: usize) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            create_private_dir(parent)?;
        }
        let file = open_private_append(path)?;
        let written = file.metadata()?.len();

        Ok(Self {
            state: Arc::new(Mutex::new(FileState {
                file,
                path: path.to_owned(),
                written,
                max_bytes,
                max_files: max_files.max(1),
                pending: Vec::new(),
            })),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FileState> {
        // A panic in another thread must not lose the log; the file handle stays valid.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl FileState {
    /// `morphix.log` → `morphix.log.1`, `morphix.log.1` → `morphix.log.2`, and so on.
    fn numbered(&self, index: usize) -> PathBuf {
        let mut name = self.path.file_name().map_or_else(
            || std::ffi::OsString::from(LOG_FILE_NAME),
            ToOwned::to_owned,
        );
        name.push(format!(".{index}"));
        self.path.with_file_name(name)
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;

        if self.max_files <= 1 {
            // No history requested: start the active file over.
            self.file = open_private_truncate(&self.path)?;
            self.written = 0;
            return Ok(());
        }

        for index in (2..self.max_files).rev() {
            let from = self.numbered(index - 1);
            if from.exists() {
                fs::rename(&from, self.numbered(index))?;
            }
        }
        if self.path.exists() {
            fs::rename(&self.path, self.numbered(1))?;
        }

        self.file = open_private_append(&self.path)?;
        self.written = 0;
        Ok(())
    }

    /// Writes one complete line, rotating first if the line would exceed the budget.
    fn emit_line(&mut self, line: &[u8]) -> io::Result<()> {
        let exceeds = self.max_bytes > 0
            && self.written > 0
            && self.written.saturating_add(line.len() as u64) > self.max_bytes;
        if exceeds {
            self.rotate()?;
        }

        self.file.write_all(line)?;
        self.written = self.written.saturating_add(line.len() as u64);
        Ok(())
    }

    /// Drains every complete line currently buffered.
    fn drain_complete_lines(&mut self) -> io::Result<()> {
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            self.emit_line(&line)?;
        }
        Ok(())
    }
}

impl Write for RotatingLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = self.lock();
        state.pending.extend_from_slice(buf);

        state.drain_complete_lines()?;

        // An over-long line without a newline is flushed rather than buffered forever.
        if state.pending.len() >= MAX_PENDING_BYTES {
            let line = std::mem::take(&mut state.pending);
            state.emit_line(&line)?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.lock();
        if !state.pending.is_empty() {
            let line = std::mem::take(&mut state.pending);
            state.emit_line(&line)?;
        }
        state.file.flush()
    }
}

impl<'a> MakeWriter<'a> for RotatingLog {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// A `tracing` field formatter that replaces sensitive values before they are written.
///
/// Non-sensitive fields and the event message are written unchanged, so log lines stay
/// greppable (`transaction_id=… target=…`).
#[derive(Debug, Clone, Copy)]
struct RedactingFields;

impl<'writer> FormatFields<'writer> for RedactingFields {
    fn format_fields<R: RecordFields>(
        &self,
        writer: Writer<'writer>,
        fields: R,
    ) -> std::fmt::Result {
        let mut visitor = RedactingVisitor {
            writer,
            first: true,
        };
        fields.record(&mut visitor);
        Ok(())
    }
}

struct RedactingVisitor<'writer> {
    writer: Writer<'writer>,
    first: bool,
}

impl RedactingVisitor<'_> {
    /// Writes `name=value`, or `name=[redacted]` for a sensitive field. The message field
    /// is written bare, without a `message=` prefix.
    fn write_field<F>(&mut self, name: &str, value: F) -> std::fmt::Result
    where
        F: FnOnce(&mut Writer<'_>) -> std::fmt::Result,
    {
        if name != "message" {
            if !self.first {
                self.writer.write_str(" ")?;
            }
            self.first = false;
            self.writer.write_str(name)?;
            self.writer.write_str("=")?;
        }

        if is_redacted_field(name) {
            self.writer.write_str(REDACTION_PLACEHOLDER)
        } else {
            value(&mut self.writer)
        }
    }
}

impl tracing::field::Visit for RedactingVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        // A broken writer is reported when the subscriber flushes; field formatting
        // itself has no error channel.
        let _ = self.write_field(field.name(), |writer| write!(writer, "{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        let _ = self.write_field(field.name(), |writer| write!(writer, "{value}"));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        let _ = self.write_field(field.name(), |writer| write!(writer, "{value}"));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        let _ = self.write_field(field.name(), |writer| write!(writer, "{value}"));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        let _ = self.write_field(field.name(), |writer| write!(writer, "{value}"));
    }
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

#[cfg(unix)]
fn open_private_append(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(unix)]
fn open_private_truncate(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private_truncate(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
}

#[cfg(test)]
mod tests {
    // Tests fail loudly: `unwrap`/`expect` with a message is allowed here even though the
    // library target denies them (CONTRIBUTING.md).
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;

    use super::{LogOptions, REDACTION_PLACEHOLDER, RotatingLog, is_redacted_field, log_path};

    /// A `MakeWriter` that captures the formatted log lines in memory.
    #[derive(Clone, Default)]
    struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

    impl CaptureWriter {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn log_path_is_under_the_state_directory() {
        assert_eq!(
            log_path(std::path::Path::new("/state/morphix")),
            std::path::Path::new("/state/morphix/logs/morphix.log")
        );
    }

    #[test]
    fn redaction_list_matches_by_name_and_is_case_insensitive() {
        assert!(is_redacted_field("config_value"));
        assert!(is_redacted_field("File_Contents"));
        assert!(!is_redacted_field("target"));
        assert!(!is_redacted_field("error_code"));
    }

    #[test]
    fn sensitive_fields_and_the_file_body_never_reach_the_log() {
        let capture = CaptureWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .fmt_fields(super::RedactingFields)
            .finish();

        let secret_body = "PRIVATE-KEY-MATERIAL-do-not-log";
        let secret_value = "Circles,Blue,Green";

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                event = "config.write",
                transaction_id = 42_u64,
                config_value = secret_value,
                file_contents = secret_body,
                target = "/home/tester/.config/kdeglobals",
            );
        });

        let output = capture.contents();
        assert!(!output.contains(secret_body), "file body leaked: {output}");
        assert!(
            !output.contains(secret_value),
            "config value leaked: {output}"
        );
        assert!(output.contains(REDACTION_PLACEHOLDER), "{output}");
        // Non-sensitive context must still be present so logs remain useful.
        assert!(output.contains("transaction_id=42"), "{output}");
        assert!(
            output.contains("target=/home/tester/.config/kdeglobals"),
            "{output}"
        );
        assert!(output.contains("event=config.write"), "{output}");
    }

    #[test]
    fn the_active_log_file_is_mode_0600() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("logs/morphix.log");

        let mut log = RotatingLog::open(&path, 0, 3).expect("open log");
        log.write_all(b"line\n").expect("write");
        log.flush().expect("flush");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("log metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "log file mode is {mode:o}");
        }
    }

    #[test]
    fn rotation_keeps_the_configured_number_of_files() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("logs/morphix.log");

        let mut log = RotatingLog::open(&path, 8, 3).expect("open log");
        for index in 0..10 {
            // Each line exceeds the 8-byte budget, so every write rotates.
            writeln!(log, "line-{index:04}").expect("write");
        }
        log.flush().expect("flush");

        assert!(path.exists(), "active log missing");
        assert!(path.with_file_name("morphix.log.1").exists());
        assert!(path.with_file_name("morphix.log.2").exists());
        assert!(
            !path.with_file_name("morphix.log.3").exists(),
            "rotation kept more files than configured"
        );

        let newest = std::fs::read_to_string(path.with_file_name("morphix.log.1")).unwrap();
        assert!(
            newest.contains("line-0008"),
            "unexpected newest file: {newest}"
        );
    }

    #[test]
    fn options_default_to_the_documented_path_and_rotation() {
        let options = LogOptions::new("/state/morphix");
        assert_eq!(
            options.file_path(),
            std::path::Path::new("/state/morphix/logs/morphix.log")
        );
        assert_eq!(options.max_files, 5);
        assert_eq!(options.max_bytes, 1024 * 1024);
    }
}
