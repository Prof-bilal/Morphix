//! Shared value types: identifiers, versions, capabilities, resources, clock.
//!
//! **Ownership.** Types used by more than one domain module, with no I/O and no
//! knowledge of the filesystem, the desktop, or storage.
//!
//! **Allowed dependencies:** `errors`, `serde`, `semver`.
//! **Must never depend on:** `fs`, `desktop`, `storage`, `planning` (ARCHITECTURE.md §5).
//!
//! **Filled in by:** P2-02 (`ids.rs`), P1-07 (`versions.rs`), P2-07 (`capability.rs`),
//! `resource.rs`, `clock.rs`.

pub mod capability;
pub mod versions;
