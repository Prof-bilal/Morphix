//! Embedded, immutable migration files (DATABASE.md §8).
//!
//! Files are compiled into the binary and applied in order. A released file is never
//! edited: the runner records its SHA-256 and refuses a database whose recorded checksum
//! no longer matches.

/// One migration: a sequential version, a description, and its SQL.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Schema version written to `schema_migrations`.
    pub version: i64,
    /// Short description stored with the version.
    pub description: &'static str,
    /// Immutable SQL text.
    pub sql: &'static str,
}

/// Every migration known to this build, oldest first.
pub const MIGRATIONS: [Migration; 1] = [Migration {
    version: 1,
    description: "initial schema",
    sql: include_str!("migrations/0001_init.sql"),
}];

/// The newest schema version this build understands.
pub const LATEST_VERSION: i64 = 1;
