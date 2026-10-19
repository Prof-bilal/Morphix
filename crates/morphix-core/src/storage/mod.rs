//! SQLite persistence: connection handling, migrations, and repositories.
//!
//! **Ownership.** The database schema and every statement that touches it. Callers pass
//! typed values; SQL lives here (DATABASE.md).
//!
//! **Allowed dependencies:** `errors`, `core`.
//! **Must never depend on:** `desktop`, `planning`, `transactions`.
//!
//! **Filled in by:** P2-01 (`db.rs`, `migrations/0001_init.sql`), P2-11
//! (`repo/packs.rs`), P2 (`repo/settings.rs`), P5-02 (`repo/transactions.rs`), P4-10
//! (`repo/snapshots.rs`), P5-13 (`repo/history.rs`).
