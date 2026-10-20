//! Morphix desktop shell: the Tauri builder, app state, typed commands, and events.
//!
//! **Scope.** This crate translates IPC requests into `morphix-core` calls and encodes the
//! results as narrow DTOs. It must not contain domain logic: validation, planning,
//! snapshots, and transactions all live in the core crate (ARCHITECTURE.md §3).

pub mod commands;
pub mod error;
pub mod events;
pub mod state;

/// Shell version, reported through `get_app_info` and shown in Diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run the desktop application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .manage(commands::SystemStatusState::new())
        .manage(commands::PackState::new())
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::system::get_system_status,
            commands::packs::list_packs,
            commands::packs::get_pack_detail,
            commands::packs::install_local_pack
        ])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        eprintln!("failed to run Morphix desktop application: {error}");
        std::process::exit(1);
    }
}
