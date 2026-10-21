//! Minimal KConfig reader for pre-state capture (Phase 4 scope: reads only).
//!
//! **Ownership.** The only place that parses KDE config file format. Writes arrive
//! with the Phase 5 apply path; this module never writes.
//!
//! **Assumption (Plasma 6, all 6.x).** KDE config files are INI-like UTF-8 text:
//! `[Group]` headers, `key=value` lines, `#`/`;` full-line comments, and `[$i]`
//! immutability markers on groups and keys. Nested groups (`[A][B]`) address the
//! `A/B` path. When a key repeats within a group, the last occurrence wins, matching
//! what KConfig itself resolves. There is no stable documented promise for the
//! repeat rule, so the capture test pins it and Phase 5 re-reads after writing
//! rather than trusting the file position.
//!
//! What this reader deliberately does not do: shell-style inline comments (a `#`
//! inside a value is data), line continuations (KConfig has none), or typed
//! interpretation (booleans, lists, and escapes stay in string form; the adapter
//! interprets them at apply time in Phase 5/6).

use crate::errors::{ErrorCode, MorphixError, Result};

/// Reads one key from KConfig-format bytes.
///
/// Returns `Ok(None)` when the group or key is absent — absence is a real pre-state
/// (`Absent`), not an error. Invalid UTF-8 is unreadable content, not absence.
///
/// # Errors
///
/// Returns `E_SNAPSHOT_UNREADABLE` when the bytes are not valid UTF-8.
pub fn read_value(bytes: &[u8], group: &str, key: &str) -> Result<Option<String>> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        MorphixError::new(
            ErrorCode::SnapshotUnreadable,
            "the config file is not valid UTF-8",
        )
        .with_context("reason", error.to_string())
    })?;
    let mut current: Option<String> = None;
    let mut found = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
            current = Some(parse_group(&line[1..line.len() - 1]));
            continue;
        }
        if current.as_deref() != Some(group) {
            continue;
        }
        let Some((raw_key, value)) = line.split_once('=') else {
            continue;
        };
        if strip_immutable(raw_key.trim()) == key {
            found = Some(value.trim().to_owned());
        }
    }
    Ok(found)
}

/// Parses a group header body into its `/`-joined path, dropping `[$i]` markers:
/// `General` → `General`, `General][$i` → `General`, `A][B` → `A/B`.
fn parse_group(body: &str) -> String {
    body.split("][")
        .map(str::trim)
        .filter(|segment| *segment != "$i" && !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Strips a trailing `[$i]` immutability marker from a key name.
fn strip_immutable(key: &str) -> &str {
    key.strip_suffix("[$i]").unwrap_or(key).trim()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::read_value;

    const SAMPLE: &[u8] = b"# a comment\n; another comment\n[General]\nColorScheme=BreezeDark\nColorScheme = Overridden\nEmpty=\n[General][$i]\nLocked=kept\n[Sub][Nested]\nDeep=yes\n";

    #[test]
    fn reads_values_comments_and_whitespace() {
        assert_eq!(
            read_value(SAMPLE, "General", "ColorScheme").expect("read"),
            Some("Overridden".to_owned()),
            "the last occurrence wins"
        );
        assert_eq!(
            read_value(SAMPLE, "General", "Empty").expect("read"),
            Some(String::new()),
            "an empty value is present, not absent"
        );
        assert_eq!(
            read_value(SAMPLE, "General", "Locked").expect("read"),
            Some("kept".to_owned()),
            "immutable markers do not hide values from capture"
        );
        assert_eq!(
            read_value(SAMPLE, "Sub/Nested", "Deep").expect("read"),
            Some("yes".to_owned())
        );
    }

    #[test]
    fn missing_groups_and_keys_are_absent() {
        assert_eq!(
            read_value(SAMPLE, "Missing", "ColorScheme").expect("read"),
            None
        );
        assert_eq!(
            read_value(SAMPLE, "General", "Missing").expect("read"),
            None
        );
        assert_eq!(
            read_value(b"", "General", "ColorScheme").expect("read"),
            None
        );
    }

    #[test]
    fn values_keep_equals_signs_and_hashes() {
        let bytes = b"[General]\nExec=run --flag=a#b\n";
        assert_eq!(
            read_value(bytes, "General", "Exec").expect("read"),
            Some("run --flag=a#b".to_owned()),
            "only the first '=' splits; '#' is data, not a comment"
        );
    }

    #[test]
    fn invalid_utf8_is_unreadable_not_absent() {
        let error =
            read_value(b"[General]\nKey=\xff\xfe\n", "General", "Key").expect_err("encoding");
        assert_eq!(error.code, crate::errors::ErrorCode::SnapshotUnreadable);
    }
}
