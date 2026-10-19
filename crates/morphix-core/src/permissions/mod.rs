//! Capability grants and consent: allowlists, manifest-request resolution, and the
//! consent token bound to a plan hash.
//!
//! **Ownership.** What a pack is allowed to change, and proof that the user agreed to the
//! exact plan that is applied.
//!
//! **Allowed dependencies:** `errors`, `core`.
//! **Must never depend on:** `fs`, `desktop`, `transactions`, `packs`.
//!
//! **Filled in by:** P2-08 (`allowlist.rs`), P2-10 (`resolve.rs`), Phase 5 (`consent.rs`).
