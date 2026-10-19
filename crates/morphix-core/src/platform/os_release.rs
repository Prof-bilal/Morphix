//! Tolerant `/etc/os-release` parsing.
//!
//! **Ownership.** Reads distribution identity from the system release file. Read-only:
//! this module never writes anything, and it never executes a command (AC-01, AC-03).
//!
//! # Tolerance contract
//!
//! `/etc/os-release` is shell-like but is *not* evaluated as a script (SECURITY.md: no
//! shell). The parser is deliberately forgiving, because a distribution's own file must
//! never be able to abort detection:
//!
//! - A missing file yields an empty [`OsRelease`] rather than an error (AC-01).
//! - Blank lines and `#` comments are ignored.
//! - A line without `=`, an empty or non-identifier key, an unterminated quote, or trailing
//!   junk after a closing quote is skipped; the remaining lines still parse.
//! - Unknown keys are ignored, and a repeated key takes its last value (shell assignment
//!   order), so a distro may add keys without breaking Morphix.
//! - Values may be unquoted, single-quoted (literal), or double-quoted (shell-style
//!   backslash escapes). An empty value clears the field.
//! - Invalid UTF-8 bytes are replaced rather than rejected: identity is informational and
//!   must never crash the app.
//!
//! The only failures are I/O errors other than "not found", and a file larger than
//! [`MAX_OS_RELEASE_BYTES`], which is refused instead of read (limit enforcement).

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

use crate::errors::{ErrorCode, MorphixError, Result};

/// Primary location, which takes precedence when present (`os-release(5)`).
pub const SYSTEM_OS_RELEASE: &str = "/etc/os-release";

/// Fallback location, used only when the primary file is missing (`os-release(5)`).
pub const FALLBACK_OS_RELEASE: &str = "/usr/lib/os-release";

/// Maximum accepted file size. Real files are under 4 KiB; 256 KiB leaves generous room
/// while refusing an unexpectedly huge file before allocating it.
pub const MAX_OS_RELEASE_BYTES: u64 = 256 * 1024;

/// Distribution identity parsed from `/etc/os-release`.
///
/// Every field is optional: a file that is missing, empty, or malformed produces a value
/// with all fields unset, and callers render that as "unknown" instead of failing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsRelease {
    /// `ID`: the machine-readable distribution identifier, for example `arch`.
    pub id: Option<String>,
    /// `NAME`: the human-readable distribution name.
    pub name: Option<String>,
    /// `PRETTY_NAME`: the name intended for user interfaces.
    pub pretty_name: Option<String>,
    /// `VERSION_ID`: the machine-readable version, when the distribution provides one.
    pub version_id: Option<String>,
    /// `ID_LIKE`: distribution IDs this one derives from, in file order.
    pub id_like: Vec<String>,
}

impl OsRelease {
    /// Parses release text. This never fails; malformed lines are skipped (AC-01).
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut release = Self::default();

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, raw_value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if !is_identifier(key) {
                continue;
            }

            let Some(value) = parse_value(raw_value.trim()) else {
                continue;
            };

            match key {
                "ID" => release.id = non_empty(value),
                "NAME" => release.name = non_empty(value),
                "PRETTY_NAME" => release.pretty_name = non_empty(value),
                "VERSION_ID" => release.version_id = non_empty(value),
                "ID_LIKE" => {
                    release.id_like = value.split_ascii_whitespace().map(str::to_owned).collect();
                }
                _ => {}
            }
        }

        release
    }

    /// Reads and parses one release file.
    ///
    /// A missing file is not an error and yields [`OsRelease::default`] (AC-01).
    ///
    /// # Errors
    ///
    /// Returns `E_IO` for an unreadable file and `E_CONFIG_TOO_LARGE` when the file exceeds
    /// [`MAX_OS_RELEASE_BYTES`].
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let text = read_optional(path.as_ref())?;
        Ok(Self::parse(text.as_deref().unwrap_or_default()))
    }

    /// Reads the system release file: `/etc/os-release`, falling back to
    /// `/usr/lib/os-release` only when the primary file is absent (`os-release(5)`).
    ///
    /// # Errors
    ///
    /// Same as [`OsRelease::from_file`].
    pub fn from_system() -> Result<Self> {
        Self::from_paths(&[Path::new(SYSTEM_OS_RELEASE), Path::new(FALLBACK_OS_RELEASE)])
    }

    /// First readable file wins; if none exists, all fields are unset.
    fn from_paths(paths: &[&Path]) -> Result<Self> {
        for path in paths {
            if let Some(text) = read_optional(path)? {
                return Ok(Self::parse(&text));
            }
        }
        Ok(Self::default())
    }

    /// Best human-readable label for the distribution: `PRETTY_NAME`, then `NAME`, then
    /// `ID`.
    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.pretty_name
            .as_deref()
            .or(self.name.as_deref())
            .or(self.id.as_deref())
    }
}

/// Reads a file with a size ceiling; `None` means the file does not exist.
fn read_optional(path: &Path) -> Result<Option<String>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(read_error(path, &error)),
    };

    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_OS_RELEASE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| read_error(path, &error))?;

    if bytes.len() as u64 > MAX_OS_RELEASE_BYTES {
        return Err(MorphixError::new(
            ErrorCode::ConfigTooLarge,
            "the system release file is larger than Morphix accepts",
        )
        .with_context("path", path.display().to_string())
        .with_context("limit_bytes", MAX_OS_RELEASE_BYTES.to_string()));
    }

    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn read_error(path: &Path, error: &std::io::Error) -> MorphixError {
    MorphixError::new(ErrorCode::Io, "failed to read the system release file")
        .with_context("path", path.display().to_string())
        .with_context("io_kind", format!("{:?}", error.kind()))
}

/// os-release keys are ASCII identifiers; anything else is a malformed line.
fn is_identifier(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Parses one raw value, returning `None` when the value is malformed.
fn parse_value(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return Some(String::new());
    }

    match raw.as_bytes()[0] {
        b'\'' => parse_single_quoted(raw),
        b'"' => parse_double_quoted(raw),
        _ => Some(unescape(raw)),
    }
}

/// Single quotes are literal; anything after the closing quote is malformed.
fn parse_single_quoted(raw: &str) -> Option<String> {
    let body = raw[1..].trim_end().strip_suffix('\'')?;
    if body.contains('\'') {
        return None;
    }
    Some(body.to_owned())
}

/// Double quotes use shell-style backslash escapes.
fn parse_double_quoted(raw: &str) -> Option<String> {
    let inner = &raw[1..];
    let mut value = String::new();
    let mut chars = inner.char_indices();

    while let Some((index, ch)) = chars.next() {
        match ch {
            '"' => {
                let rest = &inner[index + ch.len_utf8()..];
                return rest.trim().is_empty().then_some(value);
            }
            '\\' => value.push(chars.next()?.1),
            _ => value.push(ch),
        }
    }

    None // Unterminated quote.
}

/// Unquoted values: a backslash escapes the next character.
fn unescape(raw: &str) -> String {
    let mut value = String::new();
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                value.push(escaped);
            }
        } else {
            value.push(ch);
        }
    }
    value
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::path::{Path, PathBuf};

    use super::{MAX_OS_RELEASE_BYTES, OsRelease};
    use crate::errors::ErrorCode;

    const ARCH: &str = include_str!("../../../../tests/fixtures/os-release/arch.txt");
    const FEDORA: &str = include_str!("../../../../tests/fixtures/os-release/fedora.txt");
    const UBUNTU: &str = include_str!("../../../../tests/fixtures/os-release/ubuntu.txt");
    const OPENSUSE: &str = include_str!("../../../../tests/fixtures/os-release/opensuse.txt");
    const DEBIAN: &str = include_str!("../../../../tests/fixtures/os-release/debian.txt");
    const EMPTY: &str = include_str!("../../../../tests/fixtures/os-release/empty.txt");
    const MALFORMED: &str = include_str!("../../../../tests/fixtures/os-release/malformed.txt");

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/os-release")
            .join(name)
    }

    #[test]
    fn parses_arch_without_a_version_id() {
        let release = OsRelease::parse(ARCH);

        assert_eq!(release.id.as_deref(), Some("arch"));
        assert_eq!(release.name.as_deref(), Some("Arch Linux"));
        assert_eq!(release.pretty_name.as_deref(), Some("Arch Linux"));
        assert_eq!(release.version_id, None);
        assert!(release.id_like.is_empty());
    }

    #[test]
    fn parses_fedora() {
        let release = OsRelease::parse(FEDORA);

        assert_eq!(release.id.as_deref(), Some("fedora"));
        assert_eq!(release.name.as_deref(), Some("Fedora Linux"));
        assert_eq!(release.version_id.as_deref(), Some("40"));
        assert_eq!(
            release.pretty_name.as_deref(),
            Some("Fedora Linux 40 (Workstation Edition)")
        );
    }

    #[test]
    fn parses_ubuntu_and_keeps_id_like() {
        let release = OsRelease::parse(UBUNTU);

        assert_eq!(release.id.as_deref(), Some("ubuntu"));
        assert_eq!(release.version_id.as_deref(), Some("24.04"));
        assert_eq!(release.id_like, ["debian"]);
    }

    #[test]
    fn parses_opensuse_with_two_id_like_entries() {
        let release = OsRelease::parse(OPENSUSE);

        assert_eq!(release.id.as_deref(), Some("opensuse-tumbleweed"));
        assert_eq!(release.version_id.as_deref(), Some("20241014"));
        assert_eq!(release.id_like, ["opensuse", "suse"]);
    }

    #[test]
    fn parses_debian() {
        let release = OsRelease::parse(DEBIAN);

        assert_eq!(release.id.as_deref(), Some("debian"));
        assert_eq!(release.name.as_deref(), Some("Debian GNU/Linux"));
        assert_eq!(release.version_id.as_deref(), Some("12"));
        assert_eq!(
            release.display_name(),
            Some("Debian GNU/Linux 12 (bookworm)")
        );
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let release = OsRelease::from_file(dir.path().join("absent")).expect("missing is fine");

        assert_eq!(release, OsRelease::default());
        assert_eq!(release.display_name(), None);
    }

    #[test]
    fn empty_file_yields_no_fields() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("os-release");
        std::fs::write(&path, EMPTY).expect("write fixture");

        let release = OsRelease::from_file(&path).expect("empty is fine");
        assert_eq!(release, OsRelease::default());
    }

    #[test]
    fn malformed_lines_are_skipped_and_the_rest_parses() {
        let release = OsRelease::parse(MALFORMED);

        assert_eq!(release.id.as_deref(), Some("debian"));
        assert_eq!(release.version_id.as_deref(), Some("12"));
        assert_eq!(
            release.pretty_name.as_deref(),
            Some("Debian GNU/Linux 12 (bookworm)")
        );
        assert!(
            release.id_like.is_empty(),
            "the unterminated ID_LIKE is skipped"
        );
    }

    #[test]
    fn quoted_values_are_unquoted_and_escapes_are_shell_style() {
        let text = concat!(
            "NAME='Quoted Distro'\n",
            "ID=\"quoted-id\"\n",
            "PRETTY_NAME=\"Quoted \\\"Distro\\\" \\\\ Test\"\n",
            "VERSION_ID='1.0'\n",
            "ID_LIKE=\"quoted base\"\n",
        );
        let release = OsRelease::parse(text);

        assert_eq!(release.name.as_deref(), Some("Quoted Distro"));
        assert_eq!(release.id.as_deref(), Some("quoted-id"));
        assert_eq!(
            release.pretty_name.as_deref(),
            Some("Quoted \"Distro\" \\ Test")
        );
        assert_eq!(release.version_id.as_deref(), Some("1.0"));
        assert_eq!(release.id_like, ["quoted", "base"]);
    }

    #[test]
    fn unknown_keys_are_ignored_and_repeated_keys_take_the_last_value() {
        let text = "ID=first\nUNKNOWN_KEY=value\nID=second\n=\nNAME\n";
        let release = OsRelease::parse(text);

        assert_eq!(release.id.as_deref(), Some("second"));
        assert_eq!(release.name, None);
    }

    #[test]
    fn empty_value_clears_the_field() {
        let release = OsRelease::parse("ID=debian\nID=\n");

        assert_eq!(release.id, None);
    }

    #[test]
    fn carriage_returns_and_comments_are_tolerated() {
        let release = OsRelease::parse("# comment\r\nID=arch\r\nPRETTY_NAME=\"Arch Linux\"\r\n");

        assert_eq!(release.id.as_deref(), Some("arch"));
        assert_eq!(release.pretty_name.as_deref(), Some("Arch Linux"));
    }

    #[test]
    fn a_very_long_line_does_not_panic() {
        let long = "x".repeat(100_000);
        let text = format!("PRETTY_NAME=\"{long}\"\nID=long\n");
        let release = OsRelease::parse(&text);

        assert_eq!(release.id.as_deref(), Some("long"));
        assert_eq!(release.pretty_name.as_deref().unwrap().len(), long.len());
    }

    #[test]
    fn an_oversized_file_is_refused_not_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("os-release");
        std::fs::write(&path, "x".repeat(MAX_OS_RELEASE_BYTES as usize + 1)).expect("write");

        let error = OsRelease::from_file(&path).expect_err("oversized file is refused");

        assert_eq!(error.code, ErrorCode::ConfigTooLarge);
        assert!(error.context.iter().any(|entry| entry.key == "limit_bytes"));
    }

    #[test]
    fn the_fallback_file_is_used_only_when_the_primary_is_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("no-primary");
        let absent_fallback = dir.path().join("no-fallback");

        let none = OsRelease::from_paths(&[&missing, &absent_fallback]).expect("both missing");
        assert_eq!(none, OsRelease::default());

        let fallback = fixture("debian.txt");
        let release = OsRelease::from_paths(&[&missing, &fallback]).expect("fallback parses");
        assert_eq!(release.id.as_deref(), Some("debian"));

        let primary = fixture("ubuntu.txt");
        let release = OsRelease::from_paths(&[&primary, &fallback]).expect("primary wins");
        assert_eq!(release.id.as_deref(), Some("ubuntu"));
    }

    #[test]
    fn a_directory_is_reported_as_io_not_skipped() {
        let dir = tempfile::tempdir().expect("temp dir");
        let error = OsRelease::from_file(dir.path()).expect_err("a directory cannot be read");

        assert_eq!(error.code, ErrorCode::Io);
    }

    #[test]
    fn display_name_prefers_pretty_then_name_then_id() {
        let mut release = OsRelease {
            id: Some("arch".to_owned()),
            ..OsRelease::default()
        };
        assert_eq!(release.display_name(), Some("arch"));

        release.name = Some("Arch Linux".to_owned());
        assert_eq!(release.display_name(), Some("Arch Linux"));

        release.pretty_name = Some("Arch Linux (rolling)".to_owned());
        assert_eq!(release.display_name(), Some("Arch Linux (rolling)"));
    }

    #[test]
    fn path_constant_matches_the_documented_location() {
        assert_eq!(super::SYSTEM_OS_RELEASE, Path::new("/etc/os-release"));
    }
}
