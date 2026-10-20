//! Pack format: manifest models, parser, compatibility, validator, integrity, store, and
//! catalog assembly.
//!
//! **Ownership.** The trust boundary where untrusted TOML becomes validated data. Nothing
//! downstream of `packs::validator` re-parses raw pack input (SECURITY.md §4).
//!
//! **Allowed dependencies:** `errors`, `core`, `fs`.
//! **Must never depend on:** `planning`, `transactions`, `desktop`, `permissions`.
//!
//! **Filled in by:** P2-03 (`manifest.rs`), P2-04 (`parser.rs`), P1-07/P2-10
//! (`compatibility.rs`), P2-10 (`validator.rs`), P2-06 (`integrity.rs`), P2-11 (`store.rs`),
//! P2-12 (`catalog.rs`).

pub mod catalog;
pub mod compatibility;
pub mod integrity;
pub mod manifest;
pub mod parser;
pub mod path;
pub mod report;
pub mod store;
pub mod structure;
pub mod validator;
