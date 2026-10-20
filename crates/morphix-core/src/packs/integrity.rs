//! `checksums.sha256` parsing and verification (PACK_SPEC.md §7, P2-06).
//!
//! The file is untrusted text in the standard `sha256sum` format. Parsing is strict and
//! bounded; a bad line, duplicate, or entry for a nonexistent file is
//! `E_CHECKSUM_MALFORMED`, a missing entry is `E_CHECKSUM_INCOMPLETE`, and a content
//! mismatch is `E_CHECKSUM_MISMATCH`. Installation is refused on any of them.

use std::collections::BTreeMap;

use crate::errors::ErrorCode;

use super::path::validate_relative;
use crate::fs::tree::TreeSnapshot;

/// The only checksum file name in schema v1.
pub const CHECKSUM_FILE: &str = "checksums.sha256";

/// Maximum checksum file size. 2,000 files × the longest line fits well below this.
pub const MAX_CHECKSUMS_BYTES: usize = 4 * 1024 * 1024;

/// One checksum-level problem, ready for the aggregated validation report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumIssue {
    /// Stable error code.
    pub code: ErrorCode,
    /// One-based line number, when the issue is attached to a line.
    pub line: Option<usize>,
    /// Human-readable explanation with no file contents.
    pub message: String,
}

impl ChecksumIssue {
    fn new(code: ErrorCode, line: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            code,
            line,
            message: message.into(),
        }
    }
}

/// Parsed checksum entries, keyed by pack-relative path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checksums {
    entries: BTreeMap<String, String>,
}

impl Checksums {
    /// The declared hash for a path.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&str> {
        self.entries.get(path).map(String::as_str)
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no entries were parsed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Parses checksum text.
///
/// # Errors
///
/// Returns every malformed line found, capped by the caller at the report limit.
pub fn parse(text: &str) -> std::result::Result<Checksums, Vec<ChecksumIssue>> {
    if text.len() > MAX_CHECKSUMS_BYTES {
        return Err(vec![ChecksumIssue::new(
            ErrorCode::ChecksumMalformed,
            None,
            "checksums.sha256 is larger than the 4 MiB limit",
        )]);
    }

    let mut issues = Vec::new();
    let mut entries = BTreeMap::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() {
            issues.push(ChecksumIssue::new(
                ErrorCode::ChecksumMalformed,
                Some(line_number),
                "blank lines are not allowed in checksums.sha256",
            ));
            continue;
        }

        let Some((hash, rest)) = line.split_at_checked(64) else {
            issues.push(ChecksumIssue::new(
                ErrorCode::ChecksumMalformed,
                Some(line_number),
                "a checksum line must start with a 64-character SHA-256 digest",
            ));
            continue;
        };
        if !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            issues.push(ChecksumIssue::new(
                ErrorCode::ChecksumMalformed,
                Some(line_number),
                "the digest must be 64 lowercase hexadecimal characters",
            ));
            continue;
        }

        let Some(path) = rest.strip_prefix("  ").or_else(|| rest.strip_prefix(" *")) else {
            issues.push(ChecksumIssue::new(
                ErrorCode::ChecksumMalformed,
                Some(line_number),
                "the digest must be followed by two spaces or a space and an asterisk",
            ));
            continue;
        };

        if path == CHECKSUM_FILE {
            issues.push(ChecksumIssue::new(
                ErrorCode::ChecksumMalformed,
                Some(line_number),
                "checksums.sha256 must not list itself",
            ));
            continue;
        }
        if let Err(error) = validate_relative(path) {
            issues.push(ChecksumIssue::new(
                ErrorCode::ChecksumMalformed,
                Some(line_number),
                format!("invalid checksum path: {}", error.message),
            ));
            continue;
        }
        if entries
            .insert(path.to_owned(), hash.to_ascii_lowercase())
            .is_some()
        {
            issues.push(ChecksumIssue::new(
                ErrorCode::ChecksumMalformed,
                Some(line_number),
                format!("duplicate checksum entry for {path}"),
            ));
        }
    }

    if entries.is_empty() && issues.is_empty() {
        issues.push(ChecksumIssue::new(
            ErrorCode::ChecksumMalformed,
            None,
            "checksums.sha256 has no entries",
        ));
    }

    if issues.is_empty() {
        Ok(Checksums { entries })
    } else {
        Err(issues)
    }
}

/// Verifies parsed checksums against the actual tree.
///
/// The checksum file itself is excluded from the completeness requirement. Ordering is
/// deterministic: declared entries first, then missing files.
#[must_use]
pub fn verify(checksums: &Checksums, tree: &TreeSnapshot) -> Vec<ChecksumIssue> {
    let files = tree.files();
    let mut issues = Vec::new();

    for (path, expected) in checksums.entries.iter() {
        match files.get(path.as_str()) {
            None => issues.push(ChecksumIssue::new(
                ErrorCode::ChecksumMalformed,
                None,
                format!("checksums.sha256 lists a file that is not in the pack: {path}"),
            )),
            Some(entry) => {
                let actual = entry.hash.as_deref().unwrap_or("");
                if !actual.eq_ignore_ascii_case(expected) {
                    issues.push(ChecksumIssue::new(
                        ErrorCode::ChecksumMismatch,
                        None,
                        format!("content does not match the declared checksum: {path}"),
                    ));
                }
            }
        }
    }

    for path in files.keys().filter(|path| **path != CHECKSUM_FILE) {
        if checksums.get(path).is_none() {
            issues.push(ChecksumIssue::new(
                ErrorCode::ChecksumIncomplete,
                None,
                format!("checksums.sha256 does not cover {path}"),
            ));
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{Checksums, parse};

    const HASH: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    #[test]
    fn standard_lines_parse() {
        let text = format!("{HASH}  wallpapers/dark.webp\n{HASH} *pack.toml\n");
        let checksums = parse(&text).expect("valid checksums");
        assert_eq!(checksums.len(), 2);
        assert_eq!(checksums.get("wallpapers/dark.webp"), Some(HASH));
    }

    #[test]
    fn malformed_lines_are_all_reported_without_content() {
        let text = format!(
            "{HASH} wallpapers/a.webp\n{}\nnothex  wallpapers/b.webp\n{HASH}  ../escape\n{HASH}  pack.toml\n{HASH}  pack.toml\n",
            "A".repeat(64)
        );
        let issues = parse(&text).expect_err("invalid checksums");
        assert!(!issues.is_empty());
        assert!(issues.iter().all(|issue| !issue.message.contains(HASH)));
    }

    #[test]
    fn duplicate_entries_and_self_listing_are_malformed() {
        let text = format!("{HASH}  pack.toml\n{HASH}  pack.toml\n");
        assert!(parse(&text).is_err());

        let text = format!("{HASH}  checksums.sha256\n");
        assert!(parse(&text).is_err());
    }

    #[test]
    fn an_empty_file_is_malformed() {
        assert!(parse("").is_err());
        let empty = Checksums::default();
        assert!(empty.is_empty());
    }
}
