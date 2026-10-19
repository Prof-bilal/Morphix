//! Application state and the blocking-worker registry.
//!
//! **Ownership.** The single `AppState` handed to every command: core services, paths,
//! the mutation job registry, and startup-recovery status. Commands take state by
//! reference; they never construct services themselves (ARCHITECTURE.md §16: no globals).
//!
//! **Filled in by:** P7-01.
