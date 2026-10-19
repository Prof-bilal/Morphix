//! Tauri commands: the entire IPC surface.
//!
//! **Ownership.** One module per domain area, each command a thin translation of a typed
//! request into a `morphix-core` call. Commands accept IDs and small enums, never a path
//! for mutation, with the two documented exceptions: `install_local_pack` and
//! `export_diagnostics`, whose paths come from an OS dialog (ARCHITECTURE.md §13).
//!
//! **Filled in by:** P1, P2, P3 and P5 add the detection, pack, plan, and transaction
//! commands; P7-02 wires the rest.

pub mod app;

pub use app::{AppInfo, get_app_info};
