//! Snapshot and snapshot-entry persistence: the durable index (P4-10).
//!
//! **Ownership.** All SQL for `snapshots` and `snapshot_entries` (DATABASE.md §3).
//! Blobs live in the object store, never here: this module stores metadata and the
//! entry index, and the manifest file stays readable without the database.
//!
//! State discipline: rows are saved `partial` and move to `complete` only through
//! [`Storage::mark_snapshot_complete`], which the snapshot layer calls after
//! verification. Callers pass plain strings; no `snapshots` types cross this
//! boundary (ARCHITECTURE.md §5: `storage` must never depend on `snapshots`).

use rusqlite::params;

use super::packs::storage_error;
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::storage::db::Storage;

/// One persisted snapshot row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSnapshot {
    /// Snapshot id (UUIDv7 string).
    pub snapshot_id: String,
    /// Owning transaction id.
    pub transaction_id: String,
    /// `apply`, `rollback`, or `restore`.
    pub kind: String,
    /// `partial`, `complete`, or `corrupt`.
    pub state: String,
    /// Absolute path of `snapshot.json`.
    pub manifest_path: String,
    /// Sealed manifest hash; empty while `partial`.
    pub manifest_hash: String,
    /// Number of manifest entries.
    pub entry_count: i64,
    /// Total captured bytes.
    pub total_bytes: i64,
    /// Writing build version.
    pub app_version: String,
    /// RFC 3339 creation time.
    pub created_at: String,
}

/// One persisted snapshot-entry row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEntry {
    /// Owning snapshot id.
    pub snapshot_id: String,
    /// `config_file`, `config_key`, `asset_file`, `asset_dir`, or `wallpaper`.
    pub resource_type: String,
    /// Stable index string (see `ResourceRef::resource_path`).
    pub resource_path: String,
    /// Group name for config keys, empty otherwise.
    pub group_name: String,
    /// Key name for config keys, empty otherwise.
    pub key_name: String,
    /// Object hash for blob-backed pre-states, if any.
    pub object_hash: Option<String>,
    /// Canonical JSON of the pre-state.
    pub pre_state_json: String,
}

impl Storage {
    /// Saves a `partial` snapshot row and its entry index in one SQL transaction.
    /// Either the whole index lands or nothing does.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the rows cannot be written.
    pub fn save_snapshot(
        &mut self,
        snapshot: &StoredSnapshot,
        entries: &[StoredEntry],
    ) -> Result<()> {
        let transaction = self
            .connection_mut()
            .transaction()
            .map_err(|error| storage_error("begin save snapshot", &error))?;
        transaction
            .execute(
                "INSERT INTO snapshots (snapshot_id, transaction_id, kind, state, manifest_path, \
                 manifest_hash, entry_count, total_bytes, app_version, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    snapshot.snapshot_id,
                    snapshot.transaction_id,
                    snapshot.kind,
                    snapshot.state,
                    snapshot.manifest_path,
                    snapshot.manifest_hash,
                    snapshot.entry_count,
                    snapshot.total_bytes,
                    snapshot.app_version,
                    snapshot.created_at,
                ],
            )
            .map_err(|error| storage_error("insert snapshot", &error))?;
        for entry in entries {
            transaction
                .execute(
                    "INSERT INTO snapshot_entries (snapshot_id, resource_type, resource_path, \
                     group_name, key_name, object_hash, pre_state_json) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        entry.snapshot_id,
                        entry.resource_type,
                        entry.resource_path,
                        entry.group_name,
                        entry.key_name,
                        entry.object_hash,
                        entry.pre_state_json,
                    ],
                )
                .map_err(|error| storage_error("insert snapshot entry", &error))?;
        }
        transaction
            .commit()
            .map_err(|error| storage_error("commit save snapshot", &error))?;
        Ok(())
    }

    /// Moves a snapshot from `partial` to `complete` with its verified hash and
    /// counts. Only a `partial` row moves; anything else reports `false` and changes
    /// nothing, so a double completion or a completion of a corrupt row is a
    /// no-op the caller must handle, never a silent rewrite.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the update cannot run.
    pub fn mark_snapshot_complete(
        &mut self,
        snapshot_id: &str,
        manifest_hash: &str,
        entry_count: i64,
        total_bytes: i64,
    ) -> Result<bool> {
        let changed = self
            .connection_mut()
            .execute(
                "UPDATE snapshots SET state = 'complete', manifest_hash = ?2, entry_count = ?3, \
                 total_bytes = ?4 WHERE snapshot_id = ?1 AND state = 'partial'",
                params![snapshot_id, manifest_hash, entry_count, total_bytes],
            )
            .map_err(|error| storage_error("complete snapshot", &error))?;
        Ok(changed == 1)
    }

    /// Loads a snapshot row.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the row cannot be decoded.
    pub fn load_snapshot(&self, snapshot_id: &str) -> Result<Option<StoredSnapshot>> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT snapshot_id, transaction_id, kind, state, manifest_path, manifest_hash, \
                 entry_count, total_bytes, app_version, created_at FROM snapshots \
                 WHERE snapshot_id = ?1",
            )
            .map_err(|error| storage_error("prepare load snapshot", &error))?;
        let mut rows = statement
            .query([snapshot_id])
            .map_err(|error| storage_error("query load snapshot", &error))?;
        let Some(row) = rows
            .next()
            .map_err(|error| storage_error("decode load snapshot", &error))?
        else {
            return Ok(None);
        };
        Ok(Some(decode_snapshot(row)?))
    }

    /// Loads a snapshot and its entry index in entry order.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the rows cannot be decoded, and
    /// `E_INTERNAL` when the id is unknown.
    pub fn snapshot_detail(&self, snapshot_id: &str) -> Result<(StoredSnapshot, Vec<StoredEntry>)> {
        let snapshot = self.load_snapshot(snapshot_id)?.ok_or_else(|| {
            MorphixError::new(ErrorCode::Internal, "the snapshot is not recorded")
                .with_context("snapshot_id", snapshot_id)
        })?;
        let entries = self.load_snapshot_entries(snapshot_id)?;
        Ok((snapshot, entries))
    }

    /// Loads a snapshot's entry index in entry order.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the rows cannot be decoded.
    pub fn load_snapshot_entries(&self, snapshot_id: &str) -> Result<Vec<StoredEntry>> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT snapshot_id, resource_type, resource_path, group_name, key_name, \
                 object_hash, pre_state_json FROM snapshot_entries WHERE snapshot_id = ?1 \
                 ORDER BY entry_id",
            )
            .map_err(|error| storage_error("prepare load entries", &error))?;
        let mut rows = statement
            .query([snapshot_id])
            .map_err(|error| storage_error("query load entries", &error))?;
        let mut entries = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| storage_error("decode load entries", &error))?
        {
            entries.push(decode_entry(row)?);
        }
        Ok(entries)
    }

    /// Lists a transaction's snapshots, newest first.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the rows cannot be decoded.
    pub fn list_snapshots_for_transaction(
        &self,
        transaction_id: &str,
    ) -> Result<Vec<StoredSnapshot>> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT snapshot_id, transaction_id, kind, state, manifest_path, manifest_hash, \
                 entry_count, total_bytes, app_version, created_at FROM snapshots \
                 WHERE transaction_id = ?1 ORDER BY created_at DESC, snapshot_id DESC",
            )
            .map_err(|error| storage_error("prepare list snapshots", &error))?;
        let mut rows = statement
            .query([transaction_id])
            .map_err(|error| storage_error("query list snapshots", &error))?;
        let mut snapshots = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| storage_error("decode list snapshots", &error))?
        {
            snapshots.push(decode_snapshot(row)?);
        }
        Ok(snapshots)
    }
}

fn decode_snapshot(row: &rusqlite::Row<'_>) -> Result<StoredSnapshot> {
    Ok(StoredSnapshot {
        snapshot_id: row
            .get(0)
            .map_err(|error| storage_error("decode snapshot id", &error))?,
        transaction_id: row
            .get(1)
            .map_err(|error| storage_error("decode transaction id", &error))?,
        kind: row
            .get(2)
            .map_err(|error| storage_error("decode kind", &error))?,
        state: row
            .get(3)
            .map_err(|error| storage_error("decode state", &error))?,
        manifest_path: row
            .get(4)
            .map_err(|error| storage_error("decode manifest path", &error))?,
        manifest_hash: row
            .get(5)
            .map_err(|error| storage_error("decode manifest hash", &error))?,
        entry_count: row
            .get(6)
            .map_err(|error| storage_error("decode count", &error))?,
        total_bytes: row
            .get(7)
            .map_err(|error| storage_error("decode bytes", &error))?,
        app_version: row
            .get(8)
            .map_err(|error| storage_error("decode version", &error))?,
        created_at: row
            .get(9)
            .map_err(|error| storage_error("decode created", &error))?,
    })
}

fn decode_entry(row: &rusqlite::Row<'_>) -> Result<StoredEntry> {
    Ok(StoredEntry {
        snapshot_id: row
            .get(0)
            .map_err(|error| storage_error("decode id", &error))?,
        resource_type: row
            .get(1)
            .map_err(|error| storage_error("decode type", &error))?,
        resource_path: row
            .get(2)
            .map_err(|error| storage_error("decode path", &error))?,
        group_name: row
            .get(3)
            .map_err(|error| storage_error("decode group", &error))?,
        key_name: row
            .get(4)
            .map_err(|error| storage_error("decode key", &error))?,
        object_hash: row
            .get(5)
            .map_err(|error| storage_error("decode hash", &error))?,
        pre_state_json: row
            .get(6)
            .map_err(|error| storage_error("decode state", &error))?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{StoredEntry, StoredSnapshot};
    use crate::errors::ErrorCode;
    use crate::storage::db::Storage;

    fn snapshot(id: &str) -> StoredSnapshot {
        StoredSnapshot {
            snapshot_id: id.to_owned(),
            transaction_id: "txn-1".to_owned(),
            kind: "apply".to_owned(),
            state: "partial".to_owned(),
            manifest_path: format!("/tmp/{id}/snapshot.json"),
            manifest_hash: String::new(),
            entry_count: 0,
            total_bytes: 0,
            app_version: "0.1.0".to_owned(),
            created_at: "2026-10-21T00:00:00Z".to_owned(),
        }
    }

    fn entry(snapshot_id: &str, path: &str) -> StoredEntry {
        StoredEntry {
            snapshot_id: snapshot_id.to_owned(),
            resource_type: "config_key".to_owned(),
            resource_path: path.to_owned(),
            group_name: "General".to_owned(),
            key_name: "ColorScheme".to_owned(),
            object_hash: None,
            pre_state_json: "{\"kind\":\"value\",\"value\":\"BreezeDark\"}".to_owned(),
        }
    }

    fn open() -> Storage {
        let storage = Storage::open_in_memory().expect("database");
        storage
            .connection()
            .execute(
                "INSERT INTO transactions (transaction_id, kind, state, app_version, started_at) \
                 VALUES ('txn-1', 'apply_pack', 'snapshotting', '0.1.0', '2026-10-21T00:00:00Z')",
                [],
            )
            .expect("stub transaction");
        storage
    }

    #[test]
    fn save_and_detail_round_trip_in_entry_order() {
        let mut storage = open();
        storage
            .save_snapshot(
                &snapshot("snap-1"),
                &[entry("snap-1", "b"), entry("snap-1", "a")],
            )
            .expect("save");

        let (loaded, entries) = storage.snapshot_detail("snap-1").expect("detail");
        assert_eq!(loaded.state, "partial");
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.resource_path.clone())
                .collect::<Vec<_>>(),
            vec!["b".to_owned(), "a".to_owned()],
            "entry order is insertion order"
        );
    }

    #[test]
    fn completion_moves_partial_to_complete_exactly_once() {
        let mut storage = open();
        storage
            .save_snapshot(&snapshot("snap-1"), &[])
            .expect("save");

        assert!(
            storage
                .mark_snapshot_complete("snap-1", &"h".repeat(64), 3, 100)
                .expect("complete")
        );
        let (loaded, _) = storage.snapshot_detail("snap-1").expect("detail");
        assert_eq!(loaded.state, "complete");
        assert_eq!(loaded.manifest_hash, "h".repeat(64));
        assert_eq!(loaded.entry_count, 3);

        assert!(
            !storage
                .mark_snapshot_complete("snap-1", &"h".repeat(64), 3, 100)
                .expect("repeat"),
            "a second completion changes nothing"
        );
        assert!(
            !storage
                .mark_snapshot_complete("snap-missing", &"h".repeat(64), 0, 0)
                .expect("missing"),
            "an unknown snapshot changes nothing"
        );
    }

    #[test]
    fn unknown_snapshots_have_no_detail() {
        let storage = Storage::open_in_memory().expect("database");
        assert_eq!(
            storage
                .snapshot_detail("snap-missing")
                .expect_err("missing")
                .code,
            ErrorCode::Internal
        );
    }

    #[test]
    fn snapshots_list_newest_first_per_transaction() {
        let mut storage = open();
        let mut first = snapshot("snap-1");
        first.created_at = "2026-10-21T00:00:00Z".to_owned();
        let mut second = snapshot("snap-2");
        second.created_at = "2026-10-21T00:01:00Z".to_owned();
        storage.save_snapshot(&second, &[]).expect("save second");
        storage.save_snapshot(&first, &[]).expect("save first");

        let listed = storage
            .list_snapshots_for_transaction("txn-1")
            .expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].snapshot_id, "snap-2");
        assert!(
            storage
                .list_snapshots_for_transaction("txn-9")
                .expect("empty")
                .is_empty()
        );
    }
}
