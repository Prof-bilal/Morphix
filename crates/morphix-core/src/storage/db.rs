//! SQLite connection ownership, pragmas, quick check, and the migration runner.
//!
//! **Ownership.** This module, [`super::repo`], and the embedded migration files are the
//! only place SQL exists (ARCHITECTURE.md §5). Every statement is parameterized; pack text
//! is never interpolated into SQL or used as an identifier (DATABASE.md §6).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use crate::errors::{ErrorCode, MorphixError, Result};

use super::migrations::{LATEST_VERSION, MIGRATIONS, Migration};

/// The only SQLite file name.
pub const DATABASE_FILE: &str = "morphix.sqlite3";

/// One owner of the single application connection (DATABASE.md §7).
pub struct Storage {
    connection: Connection,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Storage")
    }
}

impl Storage {
    /// Opens (and migrates) the database at `path`.
    ///
    /// # Errors
    ///
    /// Returns `E_DB_CORRUPT` when `quick_check` fails or a recorded migration checksum no
    /// longer matches, `E_DB_NEWER_THAN_APP` when the file was written by a newer Morphix,
    /// and a typed I/O/internal error for filesystem or SQL failures.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
            restrict_directory(parent)?;
        }

        let mut connection = Connection::open(path).map_err(|error| sql_error("open", &error))?;
        configure(&connection)?;
        quick_check(&connection)?;
        migrate(&mut connection, Some(path))?;
        Ok(Self { connection })
    }

    /// Opens an isolated in-memory database for tests.
    ///
    /// # Errors
    ///
    /// As [`Storage::open`], minus filesystem and backup errors.
    pub fn open_in_memory() -> Result<Self> {
        let mut connection =
            Connection::open_in_memory().map_err(|error| sql_error("open memory", &error))?;
        configure(&connection)?;
        quick_check(&connection)?;
        migrate(&mut connection, None)?;
        Ok(Self { connection })
    }

    /// The single connection. Only `storage` submodules may use it.
    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Mutable access for a short write transaction.
    pub(crate) fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

fn configure(connection: &Connection) -> Result<()> {
    // WAL allows concurrent readers; V0.1 still serializes writes through this connection
    // and the mutation lock. `FULL` is chosen because journal durability precedes mutation.
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| sql_error("journal_mode", &error))?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| sql_error("foreign_keys", &error))?;
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|error| sql_error("synchronous", &error))?;
    connection
        .pragma_update(None, "busy_timeout", 5000)
        .map_err(|error| sql_error("busy_timeout", &error))?;
    Ok(())
}

fn quick_check(connection: &Connection) -> Result<()> {
    let result: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|error| sql_error("quick_check", &error))?;
    if result != "ok" {
        return Err(MorphixError::new(
            ErrorCode::DbCorrupt,
            "the Morphix database failed its integrity check",
        )
        .with_context("quick_check", result));
    }
    Ok(())
}

fn migrate(connection: &mut Connection, path: Option<&Path>) -> Result<()> {
    let current: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| sql_error("read user_version", &error))?;

    if current > LATEST_VERSION {
        return Err(MorphixError::new(
            ErrorCode::DbNewerThanApp,
            "this database was written by a newer version of Morphix",
        )
        .with_context("database_version", current.to_string())
        .with_context("supported_version", LATEST_VERSION.to_string()));
    }
    if current < 0 {
        return Err(MorphixError::new(
            ErrorCode::DbCorrupt,
            "the database has a negative schema version",
        ));
    }

    if current == LATEST_VERSION {
        verify_recorded_migrations(connection)?;
        return Ok(());
    }

    if current > 0 {
        verify_recorded_migrations(connection)?;
    }

    if let Some(path) = path {
        backup_before_migration(connection, path, current)?;
    }

    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        apply(connection, migration)?;
    }

    verify_recorded_migrations(connection)
}

fn apply(connection: &mut Connection, migration: &Migration) -> Result<()> {
    let checksum = sha256_hex(migration.sql.as_bytes());
    let transaction = connection
        .transaction()
        .map_err(|error| sql_error("begin migration", &error))?;
    transaction
        .execute_batch(migration.sql)
        .map_err(|error| sql_error("apply migration", &error))?;
    transaction
        .execute(
            "INSERT INTO schema_migrations (version, description, applied_at, checksum) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                migration.version,
                migration.description,
                now_rfc3339(),
                checksum
            ],
        )
        .map_err(|error| sql_error("record migration", &error))?;
    transaction
        .commit()
        .map_err(|error| sql_error("commit migration", &error))
}

fn verify_recorded_migrations(connection: &Connection) -> Result<()> {
    for migration in &MIGRATIONS {
        let recorded: Option<String> = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![migration.version],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| {
                MorphixError::new(
                    ErrorCode::DbCorrupt,
                    "the schema migration table is missing or unreadable",
                )
                .with_context("reason", error.to_string())
            })?;
        let Some(recorded) = recorded else {
            return Err(MorphixError::new(
                ErrorCode::DbCorrupt,
                "the database schema version has no migration record",
            )
            .with_context("version", migration.version.to_string()));
        };
        let expected = sha256_hex(migration.sql.as_bytes());
        if recorded != expected {
            return Err(MorphixError::new(
                ErrorCode::DbCorrupt,
                "a released migration file no longer matches the database",
            )
            .with_context("version", migration.version.to_string()));
        }
    }
    Ok(())
}

fn backup_before_migration(connection: &Connection, path: &Path, current: i64) -> Result<()> {
    // A fresh file has no user tables yet; backing up an empty database is pointless and
    // would clutter the data directory on first launch.
    let tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| sql_error("inspect schema", &error))?;
    if tables == 0 {
        return Ok(());
    }

    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(path, &error)),
    };
    if metadata.len() == 0 {
        return Ok(());
    }
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let backup = parent.join(format!("backup-v{current}-{stamp}.sqlite3"));
    std::fs::copy(path, &backup).map_err(|error| io_error(&backup, &error))?;
    restrict_file(&backup)?;
    Ok(())
}

fn restrict_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(path, permissions).map_err(|error| io_error(path, &error))?;
    }
    Ok(())
}

fn restrict_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, permissions).map_err(|error| io_error(path, &error))?;
    }
    Ok(())
}

fn io_error(path: &Path, error: &std::io::Error) -> MorphixError {
    let code = if error.kind() == std::io::ErrorKind::PermissionDenied {
        ErrorCode::IoPermission
    } else {
        ErrorCode::Io
    };
    MorphixError::new(code, "a filesystem operation failed")
        .with_context("path", path.display().to_string())
        .with_context("io_kind", format!("{:?}", error.kind()))
}

/// Maps SQLite failures onto the stable taxonomy.
///
/// There is no generic database code in V0.1; a non-corruption SQL failure is an internal
/// invariant violation and must never be mistaken for pack input.
fn sql_error(operation: &'static str, error: &rusqlite::Error) -> MorphixError {
    let text = error.to_string();
    let corrupt = text.contains("database disk image is malformed")
        || text.contains("file is not a database")
        || text.contains("database corruption");
    let code = if corrupt {
        ErrorCode::DbCorrupt
    } else {
        ErrorCode::Internal
    };
    MorphixError::new(code, "a database operation failed")
        .with_context("operation", operation)
        .with_context("reason", text)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

/// The current UTC time as an RFC 3339 string.
#[must_use]
pub fn now_rfc3339() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    unix_to_rfc3339(seconds)
}

/// Formats a Unix timestamp as RFC 3339 UTC without a date-library dependency.
#[must_use]
pub fn unix_to_rfc3339(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let remainder = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = remainder / 3_600;
    let minute = (remainder % 3_600) / 60;
    let second = remainder % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// Howard Hinnant's civil-from-days algorithm (public domain), with day 0 = 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Marker so a missing row can be distinguished from a database failure.
trait OptionalExt<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// The database file path under the given directory.
#[must_use]
pub fn database_path(directory: &Path) -> PathBuf {
    directory.join(DATABASE_FILE)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{Storage, unix_to_rfc3339, verify_recorded_migrations};
    use crate::errors::ErrorCode;

    #[test]
    fn timestamps_are_rfc3339_utc() {
        assert_eq!(unix_to_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_to_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(unix_to_rfc3339(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn a_fresh_memory_database_applies_version_one() {
        let storage = Storage::open_in_memory().expect("memory database");
        let version: i64 = storage
            .connection()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("user_version");
        assert_eq!(version, 1);
        verify_recorded_migrations(storage.connection()).expect("recorded migration");
    }

    #[test]
    fn history_is_append_only_at_the_sql_level() {
        let storage = Storage::open_in_memory().expect("memory database");
        storage
            .connection()
            .execute(
                "INSERT INTO history_events (occurred_at, event_type, summary) VALUES (?1, ?2, ?3)",
                ["2026-10-20T00:00:00Z", "pack_installed", "test"],
            )
            .expect("insert");
        let update = storage.connection().execute(
            "UPDATE history_events SET summary = 'changed' WHERE event_id = 1",
            [],
        );
        assert!(update.is_err(), "history updates must abort");
        let delete = storage
            .connection()
            .execute("DELETE FROM history_events WHERE event_id = 1", []);
        assert!(delete.is_err(), "history deletes must abort");
    }

    #[test]
    fn reopening_an_existing_file_does_not_back_up_again() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("db/morphix.sqlite3");
        drop(Storage::open(&path).expect("first open"));
        drop(Storage::open(&path).expect("second open"));
        let backups = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("backup-"))
            .count();
        assert_eq!(
            backups, 0,
            "a migrated database must not be backed up again"
        );
    }

    #[test]
    fn a_newer_schema_is_refused() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("morphix.sqlite3");
        drop(Storage::open(&path).expect("create"));
        {
            let connection = rusqlite::Connection::open(&path).expect("raw open");
            connection
                .pragma_update(None, "user_version", 2)
                .expect("bump version");
        }
        let error = Storage::open(&path).expect_err("newer schema refused");
        assert_eq!(error.code, ErrorCode::DbNewerThanApp);
    }
}
