//! Morphix domain engine.
//!
//! This crate holds all domain logic: detection, pack handling, planning, snapshots, and
//! the transaction engine. It must not depend on Tauri, a GUI toolkit, or a display
//! server, so that every rule in the design documents stays testable headlessly
//! (ARCHITECTURE.md §3, §16).
//!
//! Module ownership and the allowed dependency edges are described in ARCHITECTURE.md §4
//! and §5, and are enforced by `tests/architecture.rs` (P0-05).
//!
//! # Status
//!
//! Phase 0 skeleton: the modules below declare ownership and contain no logic yet. A
//! module's nested files are created when its first task lands, not in advance.

// Library code must not panic: every failure is a typed error. These rules apply to the
// library target only, so test targets can still `unwrap` a broken assertion
// (AGENTS.md §11, CONTRIBUTING.md).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

pub mod core;
pub mod desktop;
pub mod errors;
pub mod fs;
pub mod logging;
pub mod packs;
pub mod permissions;
pub mod planning;
pub mod platform;
pub mod snapshots;
pub mod storage;
pub mod system;
pub mod transactions;

/// Crate version, as declared in `Cargo.toml`.
///
/// Reported through `get_app_info` (P0-07) and shown in Diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn crate_version_is_semver() {
        assert!(
            semver::Version::parse(VERSION).is_ok(),
            "CARGO_PKG_VERSION is not a valid semver version: {VERSION}"
        );
    }
}
