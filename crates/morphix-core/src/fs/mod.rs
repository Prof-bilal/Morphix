//! Filesystem primitives: the `FileSystem` trait, path sandboxing, atomic writes, tree
//! hashing/copy/removal, the mutation lock, and temp trees for tests.
//!
//! **Ownership.** Every filesystem operation in the product goes through here, so tests
//! can substitute a fake or a recording implementation (TESTING.md §3).
//!
//! **Allowed dependencies:** `errors`.
//! **Must never depend on:** `packs`, `desktop`, `storage`, `planning` (ARCHITECTURE.md §5).
//!
//! **Filled in by:** P4-01 (`mod.rs` trait + real implementation), P4-02 (`atomic.rs`),
//! P4-03 (`sandbox.rs`), P4-04 (`tree.rs`), P4-13 (`lock.rs`), `temp.rs`.

pub mod sandbox;
pub mod tree;
