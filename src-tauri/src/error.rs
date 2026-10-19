//! Core error → IPC error mapping.
//!
//! **Ownership.** The one place that converts a `morphix_core::errors::MorphixError` and
//! its structured `FailureReport` into the DTO shape the UI renders. The code, the
//! rollback outcome, and the recovery report must survive the conversion unchanged
//! (ARCHITECTURE.md §12, UI_SPEC.md §8).
//!
//! **Filled in by:** P7-04.
