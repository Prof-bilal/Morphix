//! Transaction and rollback engine: state machine, durable journal, executors, rollback
//! with conflict handling, startup recovery, and outcome reporting.
//!
//! **Ownership.** The only code that may mutate user state, and the only code that may
//! claim a change was reversed (TRANSACTIONS.md §6–§9).
//!
//! **Allowed dependencies:** `errors`, `core`, `planning`, `snapshots`, `desktop`, `fs`,
//! `storage`.
//! **Must never depend on:** Tauri, command DTOs.
//!
//! **Filled in by:** P5-01 (`mod.rs` state types), P5-03 (`journal.rs`), P5-04…P5-06
//! (`executor.rs`), P5-10 (`engine.rs`), P5-11 (`rollback.rs`), P5-14 (`recovery.rs`),
//! P5-15 (`outcome.rs`).
