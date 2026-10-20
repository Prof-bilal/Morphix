//! `app_settings` repository (DATABASE.md §12).
//!
//! Values are stored as JSON text and are bounded UI preferences only. No setting grants a
//! capability or weakens validation.

use rusqlite::params;

use crate::errors::{ErrorCode, MorphixError, Result};
use crate::storage::db::{Storage, now_rfc3339};

use super::packs::storage_error;

impl Storage {
    /// Reads a JSON setting value.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the query fails.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut statement = self
            .connection()
            .prepare("SELECT value_json FROM app_settings WHERE key = ?1")
            .map_err(|error| storage_error("prepare setting lookup", &error))?;
        let mut rows = statement
            .query(params![key])
            .map_err(|error| storage_error("query setting", &error))?;
        match rows
            .next()
            .map_err(|error| storage_error("read setting", &error))?
        {
            Some(row) => {
                Ok(Some(row.get(0).map_err(|error| {
                    storage_error("decode setting", &error)
                })?))
            }
            None => Ok(None),
        }
    }

    /// Inserts or replaces a JSON setting value.
    ///
    /// # Errors
    ///
    /// Returns `E_LIMIT_EXCEEDED` for a key longer than 128 bytes or a value longer than
    /// 8 KiB, and a typed database error when the write fails.
    pub fn set_setting(&mut self, key: &str, value_json: &str) -> Result<()> {
        if key.is_empty() || key.len() > 128 {
            return Err(MorphixError::new(
                ErrorCode::LimitExceeded,
                "a settings key must be 1..128 bytes",
            ));
        }
        if value_json.len() > 8 * 1024 {
            return Err(MorphixError::new(
                ErrorCode::LimitExceeded,
                "a settings value must be at most 8 KiB",
            ));
        }
        self.connection()
            .execute(
                "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, \
                 updated_at = excluded.updated_at",
                params![key, value_json, now_rfc3339()],
            )
            .map_err(|error| storage_error("write setting", &error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use crate::errors::ErrorCode;
    use crate::storage::db::Storage;

    #[test]
    fn settings_upsert() {
        let mut storage = Storage::open_in_memory().expect("database");
        assert_eq!(storage.get_setting("ui.theme").expect("read"), None);
        storage.set_setting("ui.theme", "\"dark\"").expect("write");
        assert_eq!(
            storage.get_setting("ui.theme").expect("read"),
            Some("\"dark\"".to_owned())
        );
        storage
            .set_setting("ui.theme", "\"light\"")
            .expect("update");
        assert_eq!(
            storage.get_setting("ui.theme").expect("read"),
            Some("\"light\"".to_owned())
        );
    }

    #[test]
    fn oversized_settings_are_refused() {
        let mut storage = Storage::open_in_memory().expect("database");
        let error = storage
            .set_setting("ui.theme", &"x".repeat(8 * 1024 + 1))
            .expect_err("too large");
        assert_eq!(error.code, ErrorCode::LimitExceeded);
    }
}
