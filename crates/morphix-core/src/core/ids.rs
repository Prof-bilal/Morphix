//! Validated identifiers used at domain boundaries (P2-02).
//!
//! **Ownership.** Newtypes for pack IDs, asset keys, installed names, and pack versions.
//! Validation lives here so a bare `String` never crosses a boundary with unchecked pack
//! content (AGENTS.md §11).
//!
//! The rules are PACK_SPEC.md §5.2, §5.5 and §5.5's installed-name derivation. Parsing is
//! deliberately strict: case, whitespace, and leading zeroes are not normalized.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::errors::{ErrorCode, MorphixError, Result};

/// Maximum pack ID length in bytes (PACK_SPEC.md §3).
pub const MAX_PACK_ID_BYTES: usize = 128;

/// Prefixes a pack may never use (PACK_SPEC.md §5.2).
pub const RESERVED_PACK_PREFIXES: [&str; 2] = ["org.kde.", "dev.morphix.reserved."];

/// A lowercase reverse-DNS pack identifier.
///
/// The grammar is `^[a-z][a-z0-9]*(\.[a-z0-9][a-z0-9-]*)+$`, 1..=128 bytes. At least one
/// dot is required, and the reserved prefixes are refused.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackId(String);

impl PackId {
    /// Parses and validates a pack ID.
    ///
    /// # Errors
    ///
    /// Returns `E_INVALID_ID` with the offending value and a reason in the context.
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty() {
            return Err(invalid_id(value, "the pack ID is empty"));
        }
        if value.len() > MAX_PACK_ID_BYTES {
            return Err(invalid_id(value, "the pack ID is longer than 128 bytes"));
        }
        if RESERVED_PACK_PREFIXES
            .iter()
            .any(|prefix| value.starts_with(prefix))
        {
            return Err(invalid_id(value, "the pack ID uses a reserved prefix"));
        }

        let mut segments = value.split('.');
        let Some(first) = segments.next() else {
            return Err(invalid_id(value, "the pack ID has no segments"));
        };
        if !is_first_segment(first) {
            return Err(invalid_id(
                value,
                "the first segment must start with a lowercase letter and contain only \
                 lowercase letters and digits",
            ));
        }

        let mut segment_count = 1;
        for segment in segments {
            segment_count += 1;
            if !is_later_segment(segment) {
                return Err(invalid_id(
                    value,
                    "every later segment must start with a lowercase letter or digit and \
                     contain only lowercase letters, digits, and hyphens",
                ));
            }
        }

        if segment_count < 2 {
            return Err(invalid_id(value, "the pack ID needs at least one dot"));
        }

        Ok(Self(value.to_owned()))
    }

    /// The validated ID.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The engine-derived slug: the last dotted segment, lowercased, non-alphanumerics
    /// replaced by `-` (PACK_SPEC.md §5.5).
    #[must_use]
    pub fn slug(&self) -> String {
        let last = self.0.rsplit('.').next().unwrap_or(self.0.as_str());
        last.chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect()
    }
}

impl fmt::Display for PackId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for PackId {
    type Err = MorphixError;

    fn from_str(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

impl Serialize for PackId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PackId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error as _;

        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

fn is_first_segment(segment: &str) -> bool {
    let mut characters = segment.chars();
    match characters.next() {
        Some(first) if first.is_ascii_lowercase() => {
            characters.all(|character| character.is_ascii_alphanumeric())
        }
        _ => false,
    }
}

fn is_later_segment(segment: &str) -> bool {
    let mut characters = segment.chars();
    match characters.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {
            characters.all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
        }
        _ => false,
    }
}

fn invalid_id(value: &str, reason: &'static str) -> MorphixError {
    MorphixError::new(ErrorCode::InvalidId, "the pack ID is not valid")
        .with_context("value", value)
        .with_context("reason", reason)
}

/// Maximum asset key length in bytes (PACK_SPEC.md §5.5).
pub const MAX_ASSET_KEY_BYTES: usize = 64;

/// A stable, manifest-visible asset key.
///
/// The grammar is `^[a-z][a-z0-9-]{0,63}$`; keys are unique within their asset kind.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetKey(String);

impl AssetKey {
    /// Parses and validates an asset key.
    ///
    /// # Errors
    ///
    /// Returns `E_INVALID_ID` (the registry has no asset-key code) with the value and a
    /// reason in the context.
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty() {
            return Err(invalid_asset_key(value, "the asset key is empty"));
        }
        if value.len() > MAX_ASSET_KEY_BYTES {
            return Err(invalid_asset_key(
                value,
                "the asset key is longer than 64 bytes",
            ));
        }

        let mut characters = value.chars();
        match characters.next() {
            Some(first) if first.is_ascii_lowercase() => {
                if characters.all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
                }) {
                    Ok(Self(value.to_owned()))
                } else {
                    Err(invalid_asset_key(
                        value,
                        "an asset key contains characters outside lowercase letters, digits, \
                         and hyphens",
                    ))
                }
            }
            _ => Err(invalid_asset_key(
                value,
                "an asset key must start with a lowercase letter",
            )),
        }
    }

    /// The validated key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for AssetKey {
    type Err = MorphixError;

    fn from_str(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

impl Serialize for AssetKey {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AssetKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error as _;

        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

fn invalid_asset_key(value: &str, reason: &'static str) -> MorphixError {
    MorphixError::new(ErrorCode::InvalidId, "the asset key is not valid")
        .with_context("value", value)
        .with_context("reason", reason)
}

/// Maximum installed-name length in bytes.
pub const MAX_INSTALLED_NAME_BYTES: usize = 192;

/// An engine-derived on-disk asset name: `<pack-slug>-<asset-key>`.
///
/// There is no public `parse` constructor: packs cannot choose this value
/// (ARCHITECTURE.md §6). It is always produced by [`InstalledName::derive`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstalledName(String);

impl InstalledName {
    /// Derives the installed name from a validated pack ID and asset key.
    ///
    /// # Errors
    ///
    /// Returns `E_INVALID_ID` if the derived name would exceed the storage limit or fail
    /// the lowercase-safe grammar. This cannot happen for a validated pair today, but is
    /// checked rather than assumed.
    pub fn derive(pack_id: &PackId, key: &AssetKey) -> Result<Self> {
        let derived = format!("{}-{}", pack_id.slug(), key.as_str());
        if derived.len() > MAX_INSTALLED_NAME_BYTES {
            return Err(invalid_id(&derived, "the installed name is too long"));
        }
        if !derived.starts_with(|character: char| {
            character.is_ascii_lowercase() || character.is_ascii_digit()
        }) || !derived.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        }) {
            return Err(invalid_id(
                &derived,
                "the installed name is not lowercase ASCII",
            ));
        }
        Ok(Self(derived))
    }

    /// The derived name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstalledName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for InstalledName {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for InstalledName {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error as _;

        let value = String::deserialize(deserializer)?;
        if value.len() > MAX_INSTALLED_NAME_BYTES
            || !value.starts_with(|character: char| {
                character.is_ascii_lowercase() || character.is_ascii_digit()
            })
            || !value.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
        {
            return Err(D::Error::custom("invalid engine-derived installed name"));
        }
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{AssetKey, InstalledName, PackId};
    use crate::errors::ErrorCode;

    #[test]
    fn valid_pack_ids_parse_and_preserve_case() {
        for value in [
            "dev.morphix.cupertino",
            "org.example.pack-1",
            "a.b",
            "com.example.9lives",
        ] {
            let id = PackId::parse(value).expect("valid pack id");
            assert_eq!(id.as_str(), value);
            assert_eq!(id.to_string(), value);
        }
    }

    #[test]
    fn pack_ids_reject_the_documented_forms() {
        for value in [
            "",
            "nodot",
            ".leading",
            "trailing.",
            "dev..morphix",
            "Dev.Morphix",
            "dev.Morphix.pack",
            "dev.morphix..pack",
            "dev.morphix.pack_",
            "1dev.morphix",
            "dev.morphix.-pack",
            "org.kde.plasma",
            "dev.morphix.reserved.thing",
        ] {
            let error = PackId::parse(value).expect_err("invalid pack id");
            assert_eq!(error.code, ErrorCode::InvalidId, "value: {value:?}");
            assert!(!error.context.is_empty());
        }
    }

    #[test]
    fn pack_id_length_is_capped_at_128_bytes() {
        let valid = format!("a.{}", "b".repeat(126));
        assert_eq!(valid.len(), 128);
        assert!(PackId::parse(&valid).is_ok());

        let invalid = format!("a.{}", "b".repeat(127));
        assert_eq!(
            PackId::parse(&invalid).expect_err("too long").code,
            ErrorCode::InvalidId
        );
    }

    #[test]
    fn slug_is_the_last_segment_lowercased_and_sanitized() {
        let id = PackId::parse("dev.morphix.cupertino").expect("id");
        assert_eq!(id.slug(), "cupertino");

        let id = PackId::parse("org.example.pack-1").expect("id");
        assert_eq!(id.slug(), "pack-1");
    }

    #[test]
    fn asset_keys_follow_the_documented_grammar() {
        for value in ["dark", "light-2", "a", "x9"] {
            assert_eq!(AssetKey::parse(value).expect("valid key").as_str(), value);
        }

        for value in ["", "Dark", "-dark", "dark_", "dark!", "9dark"] {
            assert_eq!(
                AssetKey::parse(value).expect_err("invalid key").code,
                ErrorCode::InvalidId,
                "value: {value:?}"
            );
        }

        assert!(AssetKey::parse(&"a".repeat(64)).is_ok());
        assert!(AssetKey::parse(&"a".repeat(65)).is_err());
    }

    #[test]
    fn installed_names_are_engine_derived_and_bounded() {
        let id = PackId::parse("dev.morphix.cupertino").expect("id");
        let key = AssetKey::parse("dark").expect("key");
        assert_eq!(
            InstalledName::derive(&id, &key).expect("derived").as_str(),
            "cupertino-dark"
        );

        let long_key = AssetKey::parse(&"a".repeat(64)).expect("key");
        let long_id = PackId::parse("a.b").expect("id");
        assert!(InstalledName::derive(&long_id, &long_key).is_ok());
    }

    #[test]
    fn ids_serialize_as_strings_and_reject_invalid_deserialization() {
        let id = PackId::parse("dev.morphix.cupertino").expect("id");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, "\"dev.morphix.cupertino\"");
        assert_eq!(serde_json::from_str::<PackId>(&json).expect("parse"), id);
        assert!(serde_json::from_str::<PackId>("\"Bad.Id\"").is_err());

        let key = AssetKey::parse("dark").expect("key");
        assert_eq!(serde_json::to_string(&key).expect("serialize"), "\"dark\"");
        assert!(serde_json::from_str::<AssetKey>("\"Dark\"").is_err());
    }
}
