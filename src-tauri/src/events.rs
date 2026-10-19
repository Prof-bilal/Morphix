//! Progress and lifecycle events emitted to the webview.
//!
//! **Ownership.** `morphix://transaction-progress`, `morphix://transaction-finished`, and
//! `morphix://recovery-required`, with payloads matching the TypeScript contract exactly
//! (ARCHITECTURE.md §13).
//!
//! **Filled in by:** P7-05 and P7-06.
