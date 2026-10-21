//! Plan persistence: ephemeral rows the apply path consumes (P3-09).
//!
//! **Ownership.** All SQL for the `plans` table (DATABASE.md 3). Plans are persisted so
//! they survive a UI reload; expired or discarded plans are collected on startup and
//! after apply. Callers pass plain strings; no `planning` types cross this boundary
//! (ARCHITECTURE.md 5: `storage` must never depend on `planning`).

use rusqlite::params;

use super::packs::storage_error;
use crate::errors::{ErrorCode, MorphixError, Result};
use crate::storage::db::{Storage, now_rfc3339};

/// One persisted plan row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPlan {
    /// Surrogate plan ID.
    pub plan_id: String,
    /// `apply_pack` or `restore_snapshot`.
    pub kind: String,
    /// Pack ID for pack plans.
    pub pack_id: Option<String>,
    /// Snapshot ID for restore plans.
    pub snapshot_id: Option<String>,
    /// Content hash the consent binds to.
    pub plan_hash: String,
    /// Canonical plan JSON.
    pub plan_json: String,
    /// RFC 3339 creation time.
    pub created_at: String,
    /// RFC 3339 expiry; plans refuse to apply past this point.
    pub expires_at: String,
    /// RFC 3339 consumption time, if applied.
    pub consumed_at: Option<String>,
    /// RFC 3339 discard time, if discarded.
    pub discarded_at: Option<String>,
}

impl Storage {
    /// Persists a plan row.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the row cannot be written.
    pub fn save_plan(&mut self, plan: &StoredPlan) -> Result<()> {
        self.connection_mut()
            .execute(
                "INSERT INTO plans (plan_id, kind, pack_id, snapshot_id, plan_hash, plan_json, \
                 created_at, expires_at, consumed_at, discarded_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    plan.plan_id,
                    plan.kind,
                    plan.pack_id,
                    plan.snapshot_id,
                    plan.plan_hash,
                    plan.plan_json,
                    plan.created_at,
                    plan.expires_at,
                    plan.consumed_at,
                    plan.discarded_at,
                ],
            )
            .map_err(|error| storage_error("save plan", &error))?;
        Ok(())
    }

    /// Loads a live plan: present, unconsumed, undiscarded, and unexpired.
    ///
    /// # Errors
    ///
    /// Returns `E_PLAN_EXPIRED` for expired rows and `E_INTERNAL` for consumed,
    /// discarded, or missing rows; the apply path maps those to its own refusal.
    pub fn load_live_plan(&self, plan_id: &str, now_rfc3339: &str) -> Result<StoredPlan> {
        let plan = self.load_plan(plan_id)?.ok_or_else(|| {
            MorphixError::new(ErrorCode::Internal, "the plan is no longer available")
                .with_context("plan_id", plan_id)
        })?;
        if plan.consumed_at.is_some() || plan.discarded_at.is_some() {
            return Err(
                MorphixError::new(ErrorCode::Internal, "the plan is no longer available")
                    .with_context("plan_id", plan_id),
            );
        }
        if plan.expires_at.as_str() < now_rfc3339 {
            return Err(MorphixError::new(
                ErrorCode::PlanExpired,
                "the plan expired; preview the pack again",
            )
            .with_context("plan_id", plan_id));
        }
        Ok(plan)
    }

    /// Loads a plan row regardless of liveness.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the row cannot be decoded.
    pub fn load_plan(&self, plan_id: &str) -> Result<Option<StoredPlan>> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT plan_id, kind, pack_id, snapshot_id, plan_hash, plan_json, created_at, \
                 expires_at, consumed_at, discarded_at FROM plans WHERE plan_id = ?1",
            )
            .map_err(|error| storage_error("prepare load plan", &error))?;
        let mut rows = statement
            .query([plan_id])
            .map_err(|error| storage_error("query load plan", &error))?;
        let Some(row) = rows
            .next()
            .map_err(|error| storage_error("read plan row", &error))?
        else {
            return Ok(None);
        };
        decode_plan(row).map(Some)
    }

    /// Marks a plan discarded. Discarded plans never apply.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the update cannot be written.
    pub fn discard_plan(&mut self, plan_id: &str) -> Result<bool> {
        let changed = self
            .connection_mut()
            .execute(
                "UPDATE plans SET discarded_at = ?1 WHERE plan_id = ?2 AND discarded_at IS NULL \
                 AND consumed_at IS NULL",
                params![now_rfc3339(), plan_id],
            )
            .map_err(|error| storage_error("discard plan", &error))?;
        Ok(changed > 0)
    }

    /// Marks a plan consumed after a successful apply.
    ///
    /// # Errors
    ///
    /// Returns a typed database error when the update cannot be written.
    pub fn consume_plan(&mut self, plan_id: &str) -> Result<bool> {
        let changed = self
            .connection_mut()
            .execute(
                "UPDATE plans SET consumed_at = ?1 WHERE plan_id = ?2 AND discarded_at IS NULL \
                 AND consumed_at IS NULL",
                params![now_rfc3339(), plan_id],
            )
            .map_err(|error| storage_error("consume plan", &error))?;
        Ok(changed > 0)
    }

    /// Deletes expired plans and returns the removed count (startup GC, P3-09).
    ///
    /// # Errors
    ///
    /// Returns a typed database error when expired rows cannot be removed.
    pub fn delete_expired_plans(&mut self, now_rfc3339: &str) -> Result<u64> {
        let changed = self
            .connection_mut()
            .execute("DELETE FROM plans WHERE expires_at < ?1", [now_rfc3339])
            .map_err(|error| storage_error("delete expired plans", &error))?;
        Ok(changed as u64)
    }
}

/// Decodes one plan row in column order.
fn decode_plan(row: &rusqlite::Row<'_>) -> Result<StoredPlan> {
    Ok(StoredPlan {
        plan_id: row
            .get(0)
            .map_err(|error| storage_error("decode plan id", &error))?,
        kind: row
            .get(1)
            .map_err(|error| storage_error("decode plan kind", &error))?,
        pack_id: row
            .get(2)
            .map_err(|error| storage_error("decode plan pack", &error))?,
        snapshot_id: row
            .get(3)
            .map_err(|error| storage_error("decode plan snapshot", &error))?,
        plan_hash: row
            .get(4)
            .map_err(|error| storage_error("decode plan hash", &error))?,
        plan_json: row
            .get(5)
            .map_err(|error| storage_error("decode plan json", &error))?,
        created_at: row
            .get(6)
            .map_err(|error| storage_error("decode plan created", &error))?,
        expires_at: row
            .get(7)
            .map_err(|error| storage_error("decode plan expiry", &error))?,
        consumed_at: row
            .get(8)
            .map_err(|error| storage_error("decode plan consumed", &error))?,
        discarded_at: row
            .get(9)
            .map_err(|error| storage_error("decode plan discarded", &error))?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::StoredPlan;
    use crate::errors::ErrorCode;
    use crate::storage::db::Storage;

    fn plan(id: &str) -> StoredPlan {
        StoredPlan {
            plan_id: id.to_owned(),
            kind: "apply_pack".to_owned(),
            pack_id: Some("dev.morphix.cupertino".to_owned()),
            snapshot_id: None,
            plan_hash: "a".repeat(64),
            plan_json: "{\"kind\":\"apply_pack\"}".to_owned(),
            created_at: "2026-10-21T00:00:00Z".to_owned(),
            expires_at: "2026-10-21T00:10:00Z".to_owned(),
            consumed_at: None,
            discarded_at: None,
        }
    }

    #[test]
    fn live_load_accepts_current_and_refuses_expired() {
        let mut storage = Storage::open_in_memory().expect("database");
        storage.save_plan(&plan("plan_live")).expect("save");
        let live = storage
            .load_live_plan("plan_live", "2026-10-21T00:05:00Z")
            .expect("live");
        assert_eq!(live.plan_id, "plan_live");

        let error = storage
            .load_live_plan("plan_live", "2026-10-21T00:11:00Z")
            .expect_err("expired");
        assert_eq!(error.code, ErrorCode::PlanExpired);
    }

    #[test]
    fn discard_marks_and_gc_removes_expired() {
        let mut storage = Storage::open_in_memory().expect("database");
        storage.save_plan(&plan("plan_discard")).expect("save");
        assert!(storage.discard_plan("plan_discard").expect("discard"));
        assert!(
            storage
                .load_live_plan("plan_discard", "2026-10-21T00:05:00Z")
                .is_err()
        );

        storage.save_plan(&plan("plan_old")).expect("save");
        let removed = storage
            .delete_expired_plans("2026-10-21T00:11:00Z")
            .expect("gc");
        assert_eq!(removed, 2);
        assert!(storage.load_plan("plan_old").expect("load").is_none());
    }
}
