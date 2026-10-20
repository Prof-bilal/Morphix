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

pub mod detect;
pub mod support;

pub use detect::{
    DetectionInputs, DistroInfo, SYSTEM_STATUS_TTL, SystemStatus, SystemStatusCache,
    assemble_status, detect, probe_data_dir_writable,
};
pub use support::{
    Architecture, SupportInputs, SupportLevel, SupportReason, SupportReport, evaluate,
};
