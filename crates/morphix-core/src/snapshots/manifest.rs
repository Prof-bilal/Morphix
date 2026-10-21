//! Snapshot manifest model: the typed `snapshot.json`, its canonical JSON form, and
//! self-hash verification.
//!
//! **Ownership.** The only definition of the manifest schema (TRANSACTIONS.md §6.1).
//! Readers verify before trusting: a bad self-hash or a malformed object reference is
//! `E_SNAPSHOT_CORRUPT`, never silent acceptance. The manifest is untrusted input on
//! every read — including manifests Morphix itself wrote, which may have been tampered
//! with since.
//!
//! The model covers exactly the file-backed resources Phase 4 captures. Live desktop
//! state (panels, widgets, shortcuts, effects) arrives with the Phase 6 adapter reads;
//! extending these closed enums then is a schema-version decision, not a silent widen.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::{ErrorCode, MorphixError, Result};
use crate::fs::sandbox::SafePath;
use crate::fs::{FileSystem, MAX_READ_BYTES};

use super::store::ObjectHash;

/// Manifest schema version written by this build.
pub const MANIFEST_FORMAT_VERSION: u32 = 1;
/// File name of the manifest inside a snapshot directory (TRANSACTIONS.md §6.1).
pub const MANIFEST_FILE_NAME: &str = "snapshot.json";
/// Private mode for the manifest: it names user paths.
pub const MANIFEST_FILE_MODE: u32 = 0o600;

/// What was captured, by closed kind. Paths are absolute host paths as strings;
/// [`SafePath`] validation happens at capture time, not in this model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceRef {
    /// A whole config file that will be modified.
    ConfigFile {
        /// Absolute path of the file.
        path: String,
    },
    /// One key inside a config file.
    ConfigKey {
        /// Config file identity (`kdeglobals`, `kwinrc`, …).
        file: String,
        /// Group within the file.
        group: String,
        /// Key within the group.
        key: String,
    },
    /// A single asset file that will be replaced.
    AssetFile {
        /// Absolute path of the installed file.
        path: String,
    },
    /// An asset directory that will be replaced or removed.
    AssetDir {
        /// Absolute path of the installed directory.
        path: String,
    },
    /// The wallpaper descriptor for one desktop or all desktops.
    Wallpaper {
        /// Desktop identity, or `all`.
        desktop: String,
    },
}

impl ResourceRef {
    /// The `resource_type` value stored in `snapshot_entries` (DATABASE.md §3).
    #[must_use]
    pub fn resource_type(&self) -> &'static str {
        match self {
            Self::ConfigFile { .. } => "config_file",
            Self::ConfigKey { .. } => "config_key",
            Self::AssetFile { .. } => "asset_file",
            Self::AssetDir { .. } => "asset_dir",
            Self::Wallpaper { .. } => "wallpaper",
        }
    }

    /// The stable index string stored in `resource_path` (DATABASE.md §3).
    #[must_use]
    pub fn resource_path(&self) -> String {
        match self {
            Self::ConfigFile { path } | Self::AssetFile { path } | Self::AssetDir { path } => {
                path.clone()
            }
            Self::ConfigKey { file, group, key } => format!("{file}[{group}]/{key}"),
            Self::Wallpaper { desktop } => format!("wallpaper:{desktop}"),
        }
    }
}

/// The recorded pre-state of one resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreState {
    /// The target did not exist; restoring it means removing what was created.
    Absent,
    /// A config key value, as stored in the file (string form; typed interpretation
    /// is the adapter's job).
    Value {
        /// The value read before mutation.
        value: String,
    },
    /// A whole file, stored as an object.
    File {
        /// SHA-256 of the captured content.
        sha256: String,
        /// Permission bits of the captured file.
        mode: u32,
        /// Size in bytes.
        size: u64,
    },
    /// A whole directory tree, stored as per-file objects.
    Dir {
        /// Deterministic hash of the captured tree.
        tree_hash: String,
        /// Number of files.
        files: u64,
        /// Total bytes.
        bytes: u64,
    },
    /// A wallpaper descriptor.
    Wallpaper {
        /// Wallpaper plugin id.
        plugin: String,
        /// Image URI.
        image: String,
    },
}

/// One manifest entry: what was captured and where its content lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// What was captured.
    pub resource: ResourceRef,
    /// The recorded pre-state.
    pub pre_state: PreState,
    /// Object hash in the store, when the pre-state has blob content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
}

/// The detected environment recorded alongside a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Detected Plasma version, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plasma_version: Option<String>,
    /// Session type (`wayland`, `x11`, …).
    pub session: String,
}

/// The typed `snapshot.json` (TRANSACTIONS.md §6.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotManifest {
    /// Schema version; this build writes [`MANIFEST_FORMAT_VERSION`].
    pub format_version: u32,
    /// Owning snapshot id (UUIDv7 string).
    pub snapshot_id: String,
    /// Owning transaction id (UUIDv7 string).
    pub transaction_id: String,
    /// Creation time, RFC 3339 UTC.
    pub created_at: String,
    /// Writing build version.
    pub app_version: String,
    /// Detected environment.
    pub system: SystemInfo,
    /// Captured resources, in capture order.
    pub entries: Vec<SnapshotEntry>,
    /// SHA-256 over the canonical JSON of every field above, set by [`seal`].
    pub manifest_hash: String,
}

impl SnapshotManifest {
    /// Builds an unsealed manifest. IDs must be non-empty; the entry list may be
    /// empty when a plan genuinely touches nothing.
    ///
    /// # Errors
    ///
    /// Returns `E_INTERNAL` for empty ids or an unsupported format version.
    pub fn new(
        snapshot_id: &str,
        transaction_id: &str,
        created_at: &str,
        app_version: &str,
        system: SystemInfo,
        entries: Vec<SnapshotEntry>,
    ) -> Result<Self> {
        if snapshot_id.is_empty() || transaction_id.is_empty() {
            return Err(MorphixError::new(
                ErrorCode::Internal,
                "a snapshot manifest requires snapshot and transaction ids",
            ));
        }
        Ok(Self {
            format_version: MANIFEST_FORMAT_VERSION,
            snapshot_id: snapshot_id.to_owned(),
            transaction_id: transaction_id.to_owned(),
            created_at: created_at.to_owned(),
            app_version: app_version.to_owned(),
            system,
            entries,
            manifest_hash: String::new(),
        })
    }

    /// Computes the self-hash and returns the sealed manifest.
    ///
    /// # Errors
    ///
    /// Returns `E_INTERNAL` when the manifest cannot be serialised.
    pub fn seal(mut self) -> Result<Self> {
        let canonical = self.canonical_json()?;
        self.manifest_hash = crate::fs::tree::sha256_hex(canonical.as_bytes());
        Ok(self)
    }

    /// Recomputes the self-hash and compares it, then validates every object
    /// reference. A mismatch is corruption, never a warning.
    ///
    /// # Errors
    ///
    /// Returns `E_SNAPSHOT_CORRUPT` for a hash mismatch, an unsupported format
    /// version, or a malformed object hash.
    pub fn verify(&self) -> Result<()> {
        if self.format_version != MANIFEST_FORMAT_VERSION {
            return Err(MorphixError::new(
                ErrorCode::SnapshotCorrupt,
                "the snapshot manifest version is not supported",
            )
            .with_context("format_version", self.format_version.to_string()));
        }
        let canonical = self.canonical_json()?;
        let expected = crate::fs::tree::sha256_hex(canonical.as_bytes());
        if expected != self.manifest_hash {
            return Err(MorphixError::new(
                ErrorCode::SnapshotCorrupt,
                "the snapshot manifest hash does not match its content",
            )
            .with_context("snapshot_id", &self.snapshot_id));
        }
        for entry in &self.entries {
            if let Some(object) = &entry.object {
                ObjectHash::parse(object)?;
            }
        }
        Ok(())
    }

    /// Parses and verifies untrusted manifest bytes.
    ///
    /// # Errors
    ///
    /// Returns `E_SNAPSHOT_CORRUPT` for malformed JSON, an unsupported version, or
    /// a hash mismatch.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let manifest: Self = serde_json::from_slice(bytes).map_err(|error| {
            MorphixError::new(
                ErrorCode::SnapshotCorrupt,
                "the snapshot manifest is not valid JSON",
            )
            .with_context("reason", error.to_string())
        })?;
        manifest.verify()?;
        Ok(manifest)
    }

    /// Writes the sealed manifest to `<directory>/snapshot.json` atomically.
    ///
    /// # Errors
    ///
    /// Returns `E_INTERNAL` when the manifest is not sealed, and a typed I/O error
    /// when the write fails.
    pub fn write_to<F: FileSystem>(&self, fs: &F, directory: &SafePath) -> Result<SafePath> {
        if self.manifest_hash.is_empty() {
            return Err(MorphixError::new(
                ErrorCode::Internal,
                "the snapshot manifest must be sealed before writing",
            ));
        }
        let sorted = self.sorted_json()?;
        let path = directory
            .join_relative(MANIFEST_FILE_NAME)
            .map_err(|error| {
                MorphixError::new(ErrorCode::Internal, "the manifest path is not safe")
                    .with_context("reason", error.to_string())
            })?;
        fs.write_file_atomic(&path, sorted.as_bytes(), MANIFEST_FILE_MODE)?;
        Ok(path)
    }

    /// Reads, parses, and verifies `<directory>/snapshot.json`.
    ///
    /// # Errors
    ///
    /// Returns `E_SNAPSHOT_UNREADABLE` when the file is missing and
    /// `E_SNAPSHOT_CORRUPT` when it cannot be parsed or verified.
    pub fn read_from<F: FileSystem>(fs: &F, directory: &SafePath) -> Result<Self> {
        let path = directory
            .join_relative(MANIFEST_FILE_NAME)
            .map_err(|error| {
                MorphixError::new(ErrorCode::Internal, "the manifest path is not safe")
                    .with_context("reason", error.to_string())
            })?;
        let bytes = fs.read_file(&path, MAX_READ_BYTES).map_err(|error| {
            if error.code == ErrorCode::Io {
                MorphixError::new(
                    ErrorCode::SnapshotUnreadable,
                    "the snapshot manifest is missing",
                )
                .with_context("snapshot_dir", directory.as_path().display().to_string())
            } else {
                error
            }
        })?;
        Self::parse(&bytes)
    }

    /// Sorted full JSON: every field including `manifest_hash`, with object keys
    /// sorted recursively, so the bytes on disk are deterministic too.
    fn sorted_json(&self) -> Result<String> {
        let value = serde_json::to_value(self).map_err(|error| {
            MorphixError::new(
                ErrorCode::Internal,
                "the snapshot manifest is not serialisable",
            )
            .with_context("reason", error.to_string())
        })?;
        serde_json::to_string(&sort_value(value)).map_err(|error| {
            MorphixError::new(
                ErrorCode::Internal,
                "the snapshot manifest is not serialisable",
            )
            .with_context("reason", error.to_string())
        })
    }

    /// Canonical JSON: every field except `manifest_hash`, with object keys sorted
    /// recursively. Serialisation setting differences (map ordering features) cannot
    /// change the hash because the keys are sorted here, not by the serializer.
    fn canonical_json(&self) -> Result<String> {
        let mut value = serde_json::to_value(self).map_err(|error| {
            MorphixError::new(
                ErrorCode::Internal,
                "the snapshot manifest is not serialisable",
            )
            .with_context("reason", error.to_string())
        })?;
        if let Value::Object(ref mut map) = value {
            map.remove("manifest_hash");
        }
        let sorted = sort_value(value);
        serde_json::to_string(&sorted).map_err(|error| {
            MorphixError::new(
                ErrorCode::Internal,
                "the snapshot manifest is not serialisable",
            )
            .with_context("reason", error.to_string())
        })
    }
}

/// Recursively sorts every JSON object by key. Arrays keep their order: entry order
/// is capture order and is part of what the hash protects.
fn sort_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| (key, sort_value(item)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(sort_value).collect()),
        scalar => scalar,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        MANIFEST_FORMAT_VERSION, PreState, ResourceRef, SnapshotEntry, SnapshotManifest, SystemInfo,
    };
    use crate::errors::ErrorCode;
    use crate::fs::sandbox::SafePath;
    use crate::fs::{FileSystem, RealFileSystem};

    fn manifest() -> SnapshotManifest {
        SnapshotManifest::new(
            "01900000-0000-7000-8000-000000000001",
            "01900000-0000-7000-8000-000000000002",
            "2026-10-21T00:00:00Z",
            "0.1.0",
            SystemInfo {
                plasma_version: Some("6.3.5".to_owned()),
                session: "wayland".to_owned(),
            },
            vec![
                SnapshotEntry {
                    resource: ResourceRef::ConfigKey {
                        file: "kdeglobals".to_owned(),
                        group: "General".to_owned(),
                        key: "ColorScheme".to_owned(),
                    },
                    pre_state: PreState::Value {
                        value: "BreezeDark".to_owned(),
                    },
                    object: None,
                },
                SnapshotEntry {
                    resource: ResourceRef::ConfigFile {
                        path: "/home/tester/.config/kdeglobals".to_owned(),
                    },
                    pre_state: PreState::File {
                        sha256: crate::fs::tree::sha256_hex(b"content"),
                        mode: 0o644,
                        size: 7,
                    },
                    object: Some(crate::fs::tree::sha256_hex(b"content")),
                },
            ],
        )
        .expect("manifest")
    }

    #[test]
    fn seal_verify_round_trip_is_stable() {
        let sealed = manifest().seal().expect("seal");
        sealed.verify().expect("verify");
        let again = manifest().seal().expect("seal again");
        assert_eq!(sealed.manifest_hash, again.manifest_hash);
        assert!(!sealed.manifest_hash.is_empty());
    }

    #[test]
    fn tampering_with_one_byte_breaks_verification() {
        let sealed = manifest().seal().expect("seal");
        let mut bytes = serde_json::to_vec(&sealed).expect("serialise");
        let position = bytes
            .windows(b"BreezeDark".len())
            .position(|window| window == b"BreezeDark")
            .expect("fixture value present");
        bytes[position] = b'X';
        let error = SnapshotManifest::parse(&bytes).expect_err("tampered");
        assert_eq!(error.code, ErrorCode::SnapshotCorrupt);
    }

    #[test]
    fn malformed_json_and_versions_are_corrupt() {
        assert_eq!(
            SnapshotManifest::parse(b"{not json")
                .expect_err("malformed")
                .code,
            ErrorCode::SnapshotCorrupt
        );
        let mut migrated = manifest().seal().expect("seal");
        migrated.format_version = MANIFEST_FORMAT_VERSION + 1;
        assert_eq!(
            migrated.verify().expect_err("future version").code,
            ErrorCode::SnapshotCorrupt
        );
    }

    #[test]
    fn malformed_object_hashes_are_corrupt() {
        let mut entries = manifest().entries;
        entries[1].object = Some("../escape".to_owned());
        let mut tampered = manifest();
        tampered.entries = entries;
        let sealed = tampered.seal().expect("seal");
        assert_eq!(
            sealed.verify().expect_err("bad object hash").code,
            ErrorCode::SnapshotCorrupt
        );
    }

    #[test]
    fn resource_type_strings_match_the_database_check() {
        let cases = [
            (
                ResourceRef::ConfigFile {
                    path: "/x".to_owned(),
                },
                "config_file",
                "/x",
            ),
            (
                ResourceRef::ConfigKey {
                    file: "kdeglobals".to_owned(),
                    group: "General".to_owned(),
                    key: "ColorScheme".to_owned(),
                },
                "config_key",
                "kdeglobals[General]/ColorScheme",
            ),
            (
                ResourceRef::AssetFile {
                    path: "/y".to_owned(),
                },
                "asset_file",
                "/y",
            ),
            (
                ResourceRef::AssetDir {
                    path: "/z".to_owned(),
                },
                "asset_dir",
                "/z",
            ),
            (
                ResourceRef::Wallpaper {
                    desktop: "all".to_owned(),
                },
                "wallpaper",
                "wallpaper:all",
            ),
        ];
        for (resource, resource_type, resource_path) in cases {
            assert_eq!(resource.resource_type(), resource_type);
            assert_eq!(resource.resource_path(), resource_path);
        }
    }

    #[test]
    fn write_and_read_round_trip_through_the_filesystem() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let sealed = manifest().seal().expect("seal");
        let root = SafePath::new(directory.path().join("snap-1")).expect("safe");
        fs.create_dir_all(&root, 0o700).expect("mkdir");

        let path = sealed.write_to(&fs, &root).expect("write");
        assert!(path.as_path().ends_with("snapshot.json"));

        let loaded = SnapshotManifest::read_from(&fs, &root).expect("read");
        assert_eq!(loaded, sealed);
    }

    #[test]
    fn unsealed_manifests_are_never_written() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let root = SafePath::new(directory.path().join("snap-1")).expect("safe");
        fs.create_dir_all(&root, 0o700).expect("mkdir");
        assert_eq!(
            manifest().write_to(&fs, &root).expect_err("unsealed").code,
            ErrorCode::Internal
        );
    }

    #[test]
    fn missing_manifests_are_unreadable() {
        let directory = tempfile::tempdir().expect("tempdir");
        let fs = RealFileSystem;
        let root = SafePath::new(directory.path().join("empty")).expect("safe");
        fs.create_dir_all(&root, 0o700).expect("mkdir");
        assert_eq!(
            SnapshotManifest::read_from(&fs, &root)
                .expect_err("missing")
                .code,
            ErrorCode::SnapshotUnreadable
        );
    }

    #[test]
    fn empty_ids_are_refused() {
        assert_eq!(
            SnapshotManifest::new(
                "",
                "t",
                "now",
                "0.1.0",
                SystemInfo {
                    plasma_version: None,
                    session: "wayland".to_owned(),
                },
                Vec::new()
            )
            .expect_err("empty snapshot id")
            .code,
            ErrorCode::Internal
        );
    }
}
