//! Plasma version parsing and normalization.
//!
//! **Ownership.** The typed version used by detection, compatibility, and the plan DTO.
//! No I/O and no knowledge of where the version came from.
//!
//! # Normalization
//!
//! Plasma and package managers report versions with one, two, or three numeric components
//! (`6`, `6.0`, `6.3.5`). PACK_SPEC.md §5.3 evaluates compatibility after normalizing to
//! `MAJOR.MINOR.PATCH`, so partial versions are padded with zeroes. Pre-release and build
//! metadata are preserved for display and diagnostics.
//!
//! The parser is strict about what it accepts: only ASCII digits, no leading zeroes, no
//! empty components, at most three components, and a real SemVer suffix. A probe string
//! such as `plasmashell 6.3.5` does not parse; the caller must extract the version token
//! first. Garbage, overflow, and `6.3.5.1` are rejected with `E_INVALID_VERSION`.
//!
//! # Pre-releases and compatibility
//!
//! A pre-release runtime version keeps its suffix, but [`PlasmaVersion::release`] exposes
//! the release triple. Compatibility uses the release triple because a semver range such
//! as `>=6.0, <7.0` would otherwise reject every pre-release (the `semver` crate requires
//! an explicit pre-release comparator). Pack ranges target release lines; the status card
//! still shows the full version.

use std::fmt;
use std::str::FromStr;

use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::errors::{ErrorCode, MorphixError, Result};

/// The first supported Plasma major version.
pub const MIN_PLASMA_MAJOR: u64 = 6;

/// The exclusive upper bound of the supported Plasma major version.
pub const MAX_PLASMA_MAJOR: u64 = 7;

/// A normalized KDE Plasma version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlasmaVersion(Version);

impl PlasmaVersion {
    /// Parses and normalizes a Plasma version.
    ///
    /// Accepts `MAJOR`, `MAJOR.MINOR`, or `MAJOR.MINOR.PATCH` with an optional SemVer
    /// pre-release/build suffix.
    ///
    /// # Errors
    ///
    /// Returns `E_INVALID_VERSION` for empty input, non-numeric or empty components, more
    /// than three components, leading zeroes, overflow, or an invalid SemVer suffix.
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        if text.is_empty() {
            return Err(invalid_version(text, "the version is empty"));
        }

        let (core, suffix) = split_suffix(text);
        if core.is_empty() {
            return Err(invalid_version(text, "the numeric part is empty"));
        }

        let parts: Vec<&str> = core.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return Err(invalid_version(
                text,
                "a version has one to three numeric components",
            ));
        }

        let mut numbers = [0_u64; 3];
        for (index, part) in parts.iter().enumerate() {
            numbers[index] = parse_component(text, part)?;
        }

        let normalized = format!("{}.{}.{}{suffix}", numbers[0], numbers[1], numbers[2]);
        let version = Version::parse(&normalized)
            .map_err(|_| invalid_version(text, "the SemVer suffix is invalid"))?;

        Ok(Self(version))
    }

    /// The SemVer representation.
    #[must_use]
    pub fn as_semver(&self) -> &Version {
        &self.0
    }

    /// The release triple, with pre-release and build metadata removed.
    ///
    /// Compatibility ranges use this value; see the module docs.
    #[must_use]
    pub fn release(&self) -> Version {
        Version::new(self.0.major, self.0.minor, self.0.patch)
    }

    /// Major version.
    #[must_use]
    pub fn major(&self) -> u64 {
        self.0.major
    }

    /// Minor version.
    #[must_use]
    pub fn minor(&self) -> u64 {
        self.0.minor
    }

    /// Patch version.
    #[must_use]
    pub fn patch(&self) -> u64 {
        self.0.patch
    }

    /// Whether this is a supported Plasma 6 release (`>=6.0.0, <7.0.0`).
    ///
    /// This is a version-range fact, not the support verdict: the verdict also needs the
    /// session, DBus, and adapter probes.
    #[must_use]
    pub fn is_plasma_6(&self) -> bool {
        self.0 >= Version::new(MIN_PLASMA_MAJOR, 0, 0)
            && self.0 < Version::new(MAX_PLASMA_MAJOR, 0, 0)
    }
}

impl fmt::Display for PlasmaVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for PlasmaVersion {
    type Err = MorphixError;

    fn from_str(text: &str) -> Result<Self> {
        Self::parse(text)
    }
}

impl Serialize for PlasmaVersion {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PlasmaVersion {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error as _;

        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(D::Error::custom)
    }
}

/// Splits the numeric core from an optional `-pre` or `+build` suffix.
fn split_suffix(text: &str) -> (&str, &str) {
    match text.find(['-', '+']) {
        Some(index) => (&text[..index], &text[index..]),
        None => (text, ""),
    }
}

fn parse_component(text: &str, component: &str) -> Result<u64> {
    if component.is_empty() {
        return Err(invalid_version(text, "a numeric component is empty"));
    }
    if !component.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_version(
            text,
            "a numeric component is not all digits",
        ));
    }
    if component.len() > 1 && component.starts_with('0') {
        return Err(invalid_version(
            text,
            "a numeric component has a leading zero",
        ));
    }
    component
        .parse::<u64>()
        .map_err(|_| invalid_version(text, "a numeric component overflows u64"))
}

fn invalid_version(text: &str, reason: &'static str) -> MorphixError {
    MorphixError::new(
        ErrorCode::InvalidVersion,
        "the Plasma version is not a valid version",
    )
    .with_context("value", text)
    .with_context("reason", reason)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::PlasmaVersion;
    use crate::errors::ErrorCode;

    fn parse(text: &str) -> PlasmaVersion {
        PlasmaVersion::parse(text).expect("version parses")
    }

    #[test]
    fn full_versions_are_preserved() {
        for (text, normalized) in [
            ("6.0.0", "6.0.0"),
            ("6.3.5", "6.3.5"),
            ("6.4.0", "6.4.0"),
            ("6.10.1", "6.10.1"),
            ("5.27.11", "5.27.11"),
        ] {
            let version = parse(text);
            assert_eq!(version.to_string(), normalized);
            assert_eq!(version.as_semver().to_string(), normalized);
        }
    }

    #[test]
    fn partial_versions_are_padded_with_zeroes() {
        assert_eq!(parse("6").to_string(), "6.0.0");
        assert_eq!(parse("6.0").to_string(), "6.0.0");
        assert_eq!(parse("6.4").to_string(), "6.4.0");
        assert_eq!(parse(" 6.4 ").to_string(), "6.4.0");
    }

    #[test]
    fn pre_release_and_build_metadata_are_preserved() {
        let version = parse("6.4.0-rc1");
        assert_eq!(version.to_string(), "6.4.0-rc1");
        assert_eq!(version.release().to_string(), "6.4.0");

        let version = parse("6.4-rc.2+build.7");
        assert_eq!(version.to_string(), "6.4.0-rc.2+build.7");
        assert_eq!(version.release().to_string(), "6.4.0");
    }

    #[test]
    fn accessors_and_the_plasma_6_range_use_the_normalized_value() {
        let version = parse("6.3.5");
        assert_eq!(version.major(), 6);
        assert_eq!(version.minor(), 3);
        assert_eq!(version.patch(), 5);
        assert!(version.is_plasma_6());

        assert!(parse("6.0.0").is_plasma_6());
        assert!(parse("6.99.99").is_plasma_6());
        assert!(!parse("5.27.11").is_plasma_6());
        assert!(!parse("7.0.0").is_plasma_6());
        assert!(!parse("6.0.0-rc1").is_plasma_6());
        assert!(parse("6.4.0-rc1").is_plasma_6());
    }

    #[test]
    fn malformed_versions_are_refused() {
        for text in [
            "",
            "   ",
            "garbage",
            "six",
            "v6.3.5",
            "6.",
            ".6",
            "6..3",
            "6.3.5.1",
            "6.3.5-",
            "6.3.5+",
            "6.3.5-rc..1",
            "6. 3.5",
            "6.-1.0",
            "6.0.0.0",
            "6.3.5.0",
        ] {
            let error = PlasmaVersion::parse(text).expect_err("version must be refused");
            assert_eq!(error.code, ErrorCode::InvalidVersion, "input: {text:?}");
            assert!(error.context.iter().any(|entry| entry.key == "reason"));
        }
    }

    #[test]
    fn leading_zeroes_are_refused() {
        for text in ["06.3.5", "6.03.5", "6.3.05", "00.0.0"] {
            let error = PlasmaVersion::parse(text).expect_err("leading zero");
            assert_eq!(error.code, ErrorCode::InvalidVersion, "input: {text}");
            assert!(
                error
                    .context
                    .iter()
                    .any(|entry| entry.value.contains("leading zero"))
            );
        }
    }

    #[test]
    fn overflow_is_refused_not_wrapped() {
        let error = PlasmaVersion::parse("999999999999999999999.0.0").expect_err("overflow");
        assert_eq!(error.code, ErrorCode::InvalidVersion);
        assert!(
            error
                .context
                .iter()
                .any(|entry| entry.value.contains("overflow"))
        );
    }

    #[test]
    fn from_str_matches_parse() {
        let from_trait: PlasmaVersion = "6.3".parse().expect("parses");
        assert_eq!(from_trait, parse("6.3"));
    }

    #[test]
    fn serde_round_trips_as_a_string() {
        let version = parse("6.4.0-rc1");
        let json = serde_json::to_string(&version).expect("serializes");
        assert_eq!(json, "\"6.4.0-rc1\"");

        let back: PlasmaVersion = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, version);
    }

    #[test]
    fn serde_rejects_an_invalid_version() {
        let error = serde_json::from_str::<PlasmaVersion>("\"not-a-version\"")
            .expect_err("invalid version is rejected");
        assert!(error.to_string().contains("E_INVALID_VERSION"));
    }
}
