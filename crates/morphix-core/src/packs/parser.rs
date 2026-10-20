//! Manifest parsing: bounded read, UTF-8, TOML, schema gate, strict struct mapping.
//!
//! Raw pack text is visible only here and in [`super::validator`]; every later stage
//! consumes [`Manifest`] (SECURITY.md §4). The parser never panics on pack input: all
//! failures are typed [`MorphixError`] values with a stable error code.

use sha2::{Digest, Sha256};

use serde::de::DeserializeOwned;

use crate::errors::{ErrorCode, MorphixError, Result};

use super::manifest::{Manifest, SCHEMA_VERSION_V1};

/// Maximum `pack.toml` size in bytes (PACK_SPEC.md §3).
pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// The only manifest file name.
pub const MANIFEST_FILE: &str = "pack.toml";

/// Top-level names that are always invalid in schema v1 (PACK_SPEC.md §5.1).
///
/// `panel` and `desktop_behavior` are *not* reserved: PACK_SPEC.md §6.1 defines their valid
/// schema-v1 forms (ADR-018).
pub const RESERVED_TOP_LEVEL_FIELDS: [&str; 4] = ["hooks", "layout", "permissions", "scripts"];

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// A parsed manifest and its canonical hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedManifest {
    /// The strict typed model.
    pub manifest: Manifest,
    /// SHA-256 over the canonical JSON encoding of the typed manifest.
    pub manifest_hash: String,
    /// Size of the original `pack.toml` bytes.
    pub manifest_bytes: u64,
}

/// Parses manifest bytes under the documented size cap.
///
/// # Errors
///
/// Returns `E_MANIFEST_TOO_LARGE`, `E_MANIFEST_ENCODING`, `E_MANIFEST_SYNTAX`,
/// `E_SCHEMA_VERSION_UNSUPPORTED`, `E_UNKNOWN_FIELD`, `E_MISSING_FIELD`,
/// `E_TYPE_MISMATCH`, `E_INVALID_ID`, or `E_INVALID_VERSION`.
pub fn parse_manifest(bytes: &[u8]) -> Result<ParsedManifest> {
    let manifest_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if manifest_bytes > MAX_MANIFEST_BYTES {
        return Err(MorphixError::new(
            ErrorCode::ManifestTooLarge,
            "pack.toml is larger than the 256 KiB limit",
        )
        .with_context("file", MANIFEST_FILE)
        .with_context("size_bytes", manifest_bytes.to_string()));
    }

    let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|error| {
        MorphixError::new(ErrorCode::ManifestEncoding, "pack.toml is not valid UTF-8")
            .with_context("file", MANIFEST_FILE)
            .with_context("byte_offset", error.valid_up_to().to_string())
    })?;

    let value: toml::Value = toml::from_str(text).map_err(map_toml_error)?;
    let table = value.as_table().ok_or_else(|| {
        MorphixError::new(
            ErrorCode::ManifestSyntax,
            "pack.toml must contain a TOML table",
        )
        .with_context("file", MANIFEST_FILE)
    })?;

    for reserved in RESERVED_TOP_LEVEL_FIELDS {
        if table.contains_key(reserved) {
            return Err(MorphixError::new(
                ErrorCode::UnknownField,
                "pack.toml uses a reserved field",
            )
            .with_context("field", reserved)
            .with_context(
                "reason",
                "this name is reserved and always invalid in schema v1",
            ));
        }
    }

    match table.get("schema_version") {
        None => {
            return Err(MorphixError::new(
                ErrorCode::SchemaVersionUnsupported,
                "pack.toml is missing schema_version",
            )
            .with_context("supported", SCHEMA_VERSION_V1.to_string()));
        }
        Some(toml::Value::Integer(version)) if *version == i64::from(SCHEMA_VERSION_V1) => {}
        Some(toml::Value::Integer(version)) => {
            return Err(MorphixError::new(
                ErrorCode::SchemaVersionUnsupported,
                "this pack uses a schema version Morphix does not understand",
            )
            .with_context("schema_version", version.to_string())
            .with_context("supported", SCHEMA_VERSION_V1.to_string()));
        }
        Some(other) => {
            return Err(MorphixError::new(
                ErrorCode::TypeMismatch,
                "schema_version must be an integer",
            )
            .with_context("actual_type", type_name(other)));
        }
    }

    let manifest: Manifest = deserialize(value)?;
    let manifest_hash = manifest_hash(&manifest)?;
    Ok(ParsedManifest {
        manifest,
        manifest_hash,
        manifest_bytes,
    })
}

/// Hashes the canonical JSON form of a typed manifest.
///
/// Canonical means struct field order plus `BTreeMap` asset ordering, so whitespace and
/// comment changes do not change the pinned hash, while any semantic change does.
///
/// # Errors
///
/// Returns `E_INTERNAL` if the typed model cannot be encoded; that would be a programming
/// error, not pack input.
pub fn manifest_hash(manifest: &Manifest) -> Result<String> {
    let encoded = serde_json::to_vec(manifest).map_err(|error| {
        MorphixError::new(
            ErrorCode::Internal,
            "the validated manifest could not be canonicalized",
        )
        .with_context("reason", error.to_string())
    })?;
    let mut hasher = Sha256::new();
    hasher.update(b"morphix-manifest-v1\0");
    hasher.update(&encoded);
    Ok(hex(&hasher.finalize()))
}

fn deserialize<T: DeserializeOwned>(value: toml::Value) -> Result<T> {
    T::deserialize(value).map_err(map_serde_error)
}

fn map_toml_error(error: toml::de::Error) -> MorphixError {
    let mut mapped = MorphixError::new(ErrorCode::ManifestSyntax, "pack.toml is not valid TOML");
    if let Some(span) = error.span() {
        mapped = mapped.with_context("location", format!("bytes {}..{}", span.start, span.end));
    }
    mapped
}

/// Maps a serde field error onto the documented manifest codes.
///
/// `toml` does not expose a typed error kind for unknown/missing/typed fields, so the
/// stable prefixes and the embedded Morphix code are matched. The fallback remains
/// `E_MANIFEST_SYNTAX` rather than inventing a code.
fn map_serde_error(error: toml::de::Error) -> MorphixError {
    let text = error.to_string();
    if text.contains("E_INVALID_ID") {
        return MorphixError::new(ErrorCode::InvalidId, "a manifest identifier is invalid")
            .with_context("detail", text);
    }
    if text.contains("E_INVALID_VERSION") {
        return MorphixError::new(ErrorCode::InvalidVersion, "a manifest version is invalid")
            .with_context("detail", text);
    }
    if let Some(field) = quoted_after(&text, "unknown field") {
        return MorphixError::new(ErrorCode::UnknownField, "pack.toml has an unknown field")
            .with_context("field", field);
    }
    if let Some(field) = quoted_after(&text, "missing field") {
        return MorphixError::new(
            ErrorCode::MissingField,
            "pack.toml is missing a required field",
        )
        .with_context("field", field);
    }
    if text.contains("invalid type")
        || text.contains("data did not match any variant")
        || text.contains("invalid value")
    {
        return MorphixError::new(
            ErrorCode::TypeMismatch,
            "a manifest field has the wrong TOML type",
        )
        .with_context("detail", text);
    }
    MorphixError::new(
        ErrorCode::ManifestSyntax,
        "pack.toml does not match schema v1",
    )
    .with_context("detail", text)
}

fn quoted_after(text: &str, marker: &str) -> Option<String> {
    let rest = text.split_once(marker)?.1;
    let start = rest.find('`')? + 1;
    let end = rest[start..].find('`')? + start;
    Some(rest[start..end].to_owned())
}

fn type_name(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "string",
        toml::Value::Integer(_) => "integer",
        toml::Value::Float(_) => "float",
        toml::Value::Boolean(_) => "boolean",
        toml::Value::Datetime(_) => "datetime",
        toml::Value::Array(_) => "array",
        toml::Value::Table(_) => "table",
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{MAX_MANIFEST_BYTES, manifest_hash, parse_manifest};
    use crate::errors::ErrorCode;

    fn minimal() -> String {
        r#"
schema_version = 1

[pack]
id = "dev.example.minimal"
name = "Minimal"
version = "0.1.0"
summary = "A minimal pack."
description = "A minimal pack used by the parser tests."
authors = ["Tester"]
license = "MIT"
icon = "previews/icon.png"

[compatibility]
desktop = "kde"
plasma = ">=6.0, <7.0"

[capabilities]
requires = []

[assets]

[[previews]]
image = "previews/desktop.png"
caption = "Desktop"
"#
        .to_owned()
    }

    fn code(source: &[u8]) -> ErrorCode {
        parse_manifest(source).expect_err("manifest must fail").code
    }

    #[test]
    fn a_valid_manifest_parses_and_hashes_stably() {
        let source = minimal();
        let first = parse_manifest(source.as_bytes()).expect("valid");
        let second = parse_manifest(source.as_bytes()).expect("valid");
        assert_eq!(first.manifest_hash, second.manifest_hash);
        assert_eq!(first.manifest_hash, manifest_hash(&first.manifest).unwrap());

        let reordered = format!("{source}\n");
        assert_eq!(
            parse_manifest(reordered.as_bytes()).unwrap().manifest_hash,
            first.manifest_hash,
            "insignificant whitespace must not change the canonical hash"
        );
    }

    #[test]
    fn a_utf8_bom_is_tolerated() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(minimal().as_bytes());
        assert!(parse_manifest(&bytes).is_ok());
    }

    #[test]
    fn structural_failures_map_to_documented_codes() {
        assert_eq!(code(&[0xFF, 0xFE]), ErrorCode::ManifestEncoding);
        assert_eq!(code(b"not = [valid"), ErrorCode::ManifestSyntax);
        assert_eq!(
            code(b"schema_version = 2\n"),
            ErrorCode::SchemaVersionUnsupported
        );
        assert_eq!(code(b"schema_version = \"1\"\n"), ErrorCode::TypeMismatch);

        let unknown = minimal().replace("name = \"Minimal\"", "name = \"Minimal\"\nunknown = 1");
        assert_eq!(code(unknown.as_bytes()), ErrorCode::UnknownField);

        let missing = minimal().replace("name = \"Minimal\"\n", "");
        assert_eq!(code(missing.as_bytes()), ErrorCode::MissingField);

        let bad_version = minimal().replace("version = \"0.1.0\"", "version = \"0.1\"");
        assert_eq!(code(bad_version.as_bytes()), ErrorCode::InvalidVersion);

        let bad_id = minimal().replace(
            "id = \"dev.example.minimal\"",
            "id = \"Dev.Example.Minimal\"",
        );
        assert_eq!(code(bad_id.as_bytes()), ErrorCode::InvalidId);
    }

    #[test]
    fn reserved_fields_are_refused_with_unknown_field() {
        for field in ["hooks", "layout", "permissions", "scripts"] {
            let source = format!("{}\n[{field}]\nrun = true\n", minimal());
            assert_eq!(code(source.as_bytes()), ErrorCode::UnknownField, "{field}");
        }
    }

    #[test]
    fn oversized_input_is_refused_before_reading_further() {
        let bytes = vec![b' '; usize::try_from(MAX_MANIFEST_BYTES).unwrap() + 1];
        assert_eq!(code(&bytes), ErrorCode::ManifestTooLarge);
    }
}
