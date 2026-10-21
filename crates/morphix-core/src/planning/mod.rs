//! Change planning: plan and operation models, expansion, diffs, collision checks, and
//! the planner that refuses anything it cannot express safely.
//!
//! **Ownership.** Producing a deterministic, read-only plan. Creation of a plan must not
//! mutate anything, including the plan store (TESTING.md §4.2).
//!
//! **Allowed dependencies:** `errors`, `core`, `packs`, `permissions`, `desktop`, `fs`.
//! **Must never depend on:** `transactions`, `storage`, Tauri.
//!
//! **Filled in by:** P3-01 (`plan.rs`), P3-10 (`planner.rs`), P3-04 (`expand.rs`,
//! `expand_more.rs`), P3-07 (`diff.rs`), P3-06 (`collision.rs`), P3-02 (`reads.rs`).

pub mod collision;
pub mod diff;
pub mod expand;
pub mod expand_more;
pub mod plan;
pub mod planner;
pub mod reads;
