//! Logging bootstrap: subscriber setup, log path and rotation, file mode, redaction.
//!
//! **Ownership.** The only module that configures a log destination, and the place where
//! the rule "no file contents, no secrets, structured fields only" is enforced
//! (SECURITY.md, AGENTS.md §10).
//!
//! **Allowed dependencies:** `errors` (plus the logging crates).
//!
//! **Filled in by:** P0-10 — `tracing` subscriber, log path under
//! `$XDG_STATE_HOME/morphix/logs`, mode `0600`, size-based rotation, redaction filter,
//! and a test proving file contents never reach the log.
