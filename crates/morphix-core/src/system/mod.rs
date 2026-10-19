//! Support verdict: orchestration of the environment probes and the rules that decide
//! whether this machine can be customized, with a closed set of reasons.
//!
//! **Ownership.** The answer to "can Morphix run here, and if not, why", including the
//! refusal to run as root.
//!
//! **Allowed dependencies:** `errors`, `core`, `platform`, `desktop`, `fs`.
//! **Must never depend on:** `transactions`, `packs`.
//!
//! **Filled in by:** P1-10 (`support.rs`), P1-11 (`detect.rs`).
