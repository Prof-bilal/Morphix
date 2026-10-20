//! Pack-index and installation-state repository (DATABASE.md §4, §6).
//!
//! Only integrity hashes and installation/ownership state are persisted; names,
//! descriptions, and assets are re-read from the validated pack store on every catalog
//! load (DATABASE.md §1 principle 3).

use std::collections::BTreeMap;

use rusqlite::{Row, params};

use crate::core::resource::{IntegritySource, PackSource, PackStatus};
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::storage::db::{Storage, now_rfc3339};

/// A registered pack row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPack {
    /// Manifest pack ID.
    pub pack_id: String,
    /// Strict SemVer string.
    pub version: String,
    /// Canonical manifest hash pinned at install.
    pub manifest_hash: String,
    /// Deterministic tree hash pinned at install.
    pub content_hash: String,
    /// Bundled or local.
    pub source: PackSource,
    /// Absolute pack-store directory.
    pub store_path: String,
    /// Declared checksums or an install-time generated hash.
    pub integrity_source: IntegritySource,
    /// Last known validation status.
    pub status: PackStatus,
    /// RFC 3339 installation time.
    pub installed_at: String,
    /// RFC 3339 time of the last successful validation.
    pub last_validated_at: Option<String>,
}

/// Current applied state from `pack_installations`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackInstallation {
    /// Applied or rolled back.
    pub applied: bool,
    /// Version recorded when it was applied.
    pub version: String,
    /// Transaction that produced the state.
    pub applied_transaction_id: Option<String>,
    /// RFC 3339 time of the last state change.
    pub updated_at: String,
}

impl Storage {
    /// Inserts a pack row and its `pack_installed` history event in one transaction.
    ///
    /// # Errors
    ///
    /// Returns `E_PACK_ALREADY_INSTALLED` when the ID already exists. The caller should
    /// check first so it can distinguish a version conflict.
    pub fn insert_pack(&mut self, pack: &InstalledPack) -> Result<()> {
        let transaction = self
            .connection_mut()
            .transaction()
            .map_err(|error| storage_error("begin insert pack", &error))?;
        transaction
            .execute(
                "INSERT INTO packs (pack_id, version, manifest_hash, content_hash, source, \
                 store_path, integrity_source, status, installed_at, last_validated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    pack.pack_id,
                    pack.version,
                    pack.manifest_hash,
                    pack.content_hash,
                    pack.source.as_str(),
                    pack.store_path,
                    pack.integrity_source.as_str(),
                    pack.status.as_str(),
                    pack.installed_at,
                    pack.last_validated_at,
                ],
            )
            .map_err(|error| {
                if error.to_string().contains("packs.pack_id") {
                    MorphixError::new(
                        ErrorCode::PackAlreadyInstalled,
                        "this pack is already installed",
                    )
                    .with_context("pack_id", &pack.pack_id)
                } else {
                    storage_error("insert pack", &error)
                }
            })?;
        transaction
            .execute(
                "INSERT INTO history_events (occurred_at, event_type, pack_id, summary) \
                 VALUES (?1, 'pack_installed', ?2, ?3)",
                params![
                    pack.installed_at,
                    pack.pack_id,
                    format!("Installed {} {}", pack.pack_id, pack.version)
                ],
            )
            .map_err(|error| storage_error("record pack install", &error))?;
        transaction
            .commit()
            .map_err(|error| storage_error("commit pack install", &error))
    }

    /// Reads one registered pack.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the row exists but cannot be decoded.
    pub fn get_installed_pack(&self, pack_id: &str) -> Result<Option<InstalledPack>> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT pack_id, version, manifest_hash, content_hash, source, store_path, \
                 integrity_source, status, installed_at, last_validated_at \
                 FROM packs WHERE pack_id = ?1",
            )
            .map_err(|error| storage_error("prepare pack lookup", &error))?;
        let mut rows = statement
            .query(params![pack_id])
            .map_err(|error| storage_error("query pack", &error))?;
        match rows
            .next()
            .map_err(|error| storage_error("read pack", &error))?
        {
            Some(row) => Ok(Some(decode_pack(row)?)),
            None => Ok(None),
        }
    }

    /// Lists every registered pack in ID order.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when a row cannot be decoded.
    pub fn list_installed_packs(&self) -> Result<Vec<InstalledPack>> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT pack_id, version, manifest_hash, content_hash, source, store_path, \
                 integrity_source, status, installed_at, last_validated_at \
                 FROM packs ORDER BY pack_id",
            )
            .map_err(|error| storage_error("prepare pack list", &error))?;
        let mut rows = statement
            .query([])
            .map_err(|error| storage_error("query packs", &error))?;
        let mut packs = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| storage_error("read pack list", &error))?
        {
            packs.push(decode_pack(row)?);
        }
        Ok(packs)
    }

    /// Reads current applied state per pack.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when a row cannot be decoded.
    pub fn pack_installations(&self) -> Result<BTreeMap<String, PackInstallation>> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT pack_id, version, status, applied_transaction_id, updated_at \
                 FROM pack_installations ORDER BY pack_id",
            )
            .map_err(|error| storage_error("prepare installation list", &error))?;
        let mut rows = statement
            .query([])
            .map_err(|error| storage_error("query installations", &error))?;
        let mut installations = BTreeMap::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| storage_error("read installation", &error))?
        {
            let pack_id: String = row
                .get(0)
                .map_err(|error| storage_error("decode installation", &error))?;
            let status: String = row
                .get(2)
                .map_err(|error| storage_error("decode installation", &error))?;
            installations.insert(
                pack_id,
                PackInstallation {
                    applied: status == "applied",
                    version: row
                        .get(1)
                        .map_err(|error| storage_error("decode installation", &error))?,
                    applied_transaction_id: row
                        .get(3)
                        .map_err(|error| storage_error("decode installation", &error))?,
                    updated_at: row
                        .get(4)
                        .map_err(|error| storage_error("decode installation", &error))?,
                },
            );
        }
        Ok(installations)
    }

    /// Records a rejected install without writing any pack or filesystem state.
    ///
    /// `details` must contain only codes, fields, and pack-relative paths; never manifest
    /// text or asset bytes.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the event cannot be written.
    pub fn record_rejected_install(
        &mut self,
        pack_id: Option<&str>,
        summary: &str,
        details: &serde_json::Value,
    ) -> Result<()> {
        let encoded = serde_json::to_string(details).map_err(|error| {
            MorphixError::new(
                ErrorCode::Internal,
                "the install-rejection details could not be encoded",
            )
            .with_context("reason", error.to_string())
        })?;
        let timestamp = now_rfc3339();
        let transaction = self
            .connection_mut()
            .transaction()
            .map_err(|error| storage_error("begin rejection history", &error))?;
        transaction
            .execute(
                "INSERT INTO history_events \
                 (occurred_at, event_type, pack_id, summary, details_json) \
                 VALUES (?1, 'pack_install_rejected', ?2, ?3, ?4)",
                params![timestamp, pack_id, summary, encoded],
            )
            .map_err(|error| storage_error("record rejection", &error))?;
        transaction
            .commit()
            .map_err(|error| storage_error("commit rejection history", &error))
    }
}

fn decode_pack(row: &Row<'_>) -> Result<InstalledPack> {
    let source: String = row
        .get(4)
        .map_err(|error| storage_error("decode pack source", &error))?;
    let integrity: String = row
        .get(6)
        .map_err(|error| storage_error("decode integrity source", &error))?;
    let status: String = row
        .get(7)
        .map_err(|error| storage_error("decode pack status", &error))?;
    Ok(InstalledPack {
        pack_id: row
            .get(0)
            .map_err(|error| storage_error("decode pack id", &error))?,
        version: row
            .get(1)
            .map_err(|error| storage_error("decode pack version", &error))?,
        manifest_hash: row
            .get(2)
            .map_err(|error| storage_error("decode manifest hash", &error))?,
        content_hash: row
            .get(3)
            .map_err(|error| storage_error("decode content hash", &error))?,
        source: match source.as_str() {
            "bundled" => PackSource::Bundled,
            "local" => PackSource::Local,
            other => {
                return Err(MorphixError::new(
                    ErrorCode::DbCorrupt,
                    "the pack row has an unknown source",
                )
                .with_context("source", other));
            }
        },
        store_path: row
            .get(5)
            .map_err(|error| storage_error("decode store path", &error))?,
        integrity_source: match integrity.as_str() {
            "declared" => IntegritySource::Declared,
            "generated_local" => IntegritySource::GeneratedLocal,
            other => {
                return Err(MorphixError::new(
                    ErrorCode::DbCorrupt,
                    "the pack row has an unknown integrity source",
                )
                .with_context("integrity_source", other));
            }
        },
        status: match status.as_str() {
            "valid" => PackStatus::Valid,
            "invalid" => PackStatus::Invalid,
            other => {
                return Err(MorphixError::new(
                    ErrorCode::DbCorrupt,
                    "the pack row has an unknown status",
                )
                .with_context("status", other));
            }
        },
        installed_at: row
            .get(8)
            .map_err(|error| storage_error("decode installed at", &error))?,
        last_validated_at: row
            .get(9)
            .map_err(|error| storage_error("decode last validated at", &error))?,
    })
}

pub(crate) fn storage_error(operation: &'static str, error: &rusqlite::Error) -> MorphixError {
    MorphixError::new(ErrorCode::Internal, "a database operation failed")
        .with_context("operation", operation)
        .with_context("reason", error.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::InstalledPack;
    use crate::core::resource::{IntegritySource, PackSource, PackStatus};
    use crate::errors::ErrorCode;
    use crate::storage::db::Storage;

    fn pack(id: &str, version: &str, content_hash: &str) -> InstalledPack {
        InstalledPack {
            pack_id: id.to_owned(),
            version: version.to_owned(),
            manifest_hash: "a".repeat(64),
            content_hash: content_hash.to_owned(),
            source: PackSource::Local,
            store_path: format!("/tmp/{id}"),
            integrity_source: IntegritySource::GeneratedLocal,
            status: PackStatus::Valid,
            installed_at: "2026-10-20T00:00:00Z".to_owned(),
            last_validated_at: None,
        }
    }

    #[test]
    fn packs_round_trip_and_are_listed_in_id_order() {
        let mut storage = Storage::open_in_memory().expect("database");
        storage
            .insert_pack(&pack("dev.example.b", "1.0.0", &"b".repeat(64)))
            .expect("insert b");
        storage
            .insert_pack(&pack("dev.example.a", "1.0.0", &"a".repeat(64)))
            .expect("insert a");

        let listed = storage.list_installed_packs().expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].pack_id, "dev.example.a");
        assert_eq!(
            storage
                .get_installed_pack("dev.example.b")
                .expect("get")
                .expect("present")
                .content_hash,
            "b".repeat(64)
        );
    }

    #[test]
    fn duplicate_ids_are_refused() {
        let mut storage = Storage::open_in_memory().expect("database");
        storage
            .insert_pack(&pack("dev.example.same", "1.0.0", &"a".repeat(64)))
            .expect("first");
        let error = storage
            .insert_pack(&pack("dev.example.same", "1.0.0", &"a".repeat(64)))
            .expect_err("duplicate");
        assert_eq!(error.code, ErrorCode::PackAlreadyInstalled);
    }

    #[test]
    fn rejected_installs_only_add_history() {
        let mut storage = Storage::open_in_memory().expect("database");
        storage
            .record_rejected_install(
                Some("dev.example.bad"),
                "Rejected dev.example.bad",
                &serde_json::json!({"codes": ["E_MANIFEST_SYNTAX"]}),
            )
            .expect("history");
        assert!(storage.list_installed_packs().expect("list").is_empty());
        let count: i64 = storage
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM history_events WHERE event_type = 'pack_install_rejected'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 1);
    }
}
