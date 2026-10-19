//! Host environment probing: XDG paths, `/etc/os-release`, package databases, session.
//!
//! **Ownership.** Read-only facts about the machine and the environment, and the
//! allowlisted command runner used for the two version probes.
//!
//! **Allowed dependencies:** `errors`, `core`.
//! **Must never depend on:** `planning`, `transactions`, `desktop` (ARCHITECTURE.md §5).
//!
//! **Filled in by:** P1-01 (`paths.rs`), P1-02 (`os_release.rs`), P1-03 (`package_db.rs`),
//! P1-04 (`session.rs`), P1-05 (`probe.rs`).

pub mod os_release;
pub mod package_db;
pub mod paths;
pub mod session;
