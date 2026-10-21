//! Snapshot engine: manifests, capture, the content-addressed object store, and restore
//! plan generation.
//!
//! **Ownership.** Recording exactly the resources a transaction will touch, verifiably,
//! before the first write (TRANSACTIONS.md §5).
//!
//! **Allowed dependencies:** `errors`, `core`, `fs`, `desktop`, `storage`.
//! **Must never depend on:** `planning`, `transactions`, `packs`.
//!
//! **Filled in by:** P4-06 (`manifest.rs`), P4-07/P4-08 (`capture.rs`), P4-05 (`store.rs`),
//! P4-11 (`restore.rs`).

pub mod capture;
pub mod manifest;
pub mod restore;
pub mod store;
pub mod verify;
