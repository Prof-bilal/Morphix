//! System-status IPC command.
//!
//! **Ownership.** Thin translation only: the cache and detection live in `morphix-core`.
//! Detection performs DBus and probe I/O, so it runs on Tauri's blocking worker rather
//! than the async runtime.

use std::sync::Arc;

use morphix_core::system::{SYSTEM_STATUS_TTL, SystemStatus, SystemStatusCache, detect};
use tauri::State;

/// Managed state for `get_system_status`.
///
/// P7-01 folds this into `AppState`; until then it is the minimal state the command needs.
pub struct SystemStatusState {
    cache: Arc<SystemStatusCache>,
}

impl SystemStatusState {
    /// Builds the state with the documented cache lifetime.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: Arc::new(SystemStatusCache::new(SYSTEM_STATUS_TTL)),
        }
    }
}

impl Default for SystemStatusState {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the current support verdict and detected environment.
///
/// `refresh: false` may serve a status cached for up to [`SYSTEM_STATUS_TTL`]; `refresh:
/// true` always re-detects. The frontend passes `true` from **Re-check**.
///
/// # Errors
///
/// Returns a message when detection fails (for example, an unreadable process identity or
/// a session bus that connected but could not answer). The UI shows an error banner and
/// offers Re-check; a failed result is never cached.
#[tauri::command]
pub async fn get_system_status(
    refresh: bool,
    state: State<'_, SystemStatusState>,
) -> Result<SystemStatus, String> {
    let cache = Arc::clone(&state.cache);

    tauri::async_runtime::spawn_blocking(move || cache.get_or_detect(refresh, detect))
        .await
        .map_err(|error| format!("get_system_status worker failed: {error}"))?
        .map_err(|error| format!("get_system_status failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::SystemStatusState;

    #[test]
    fn the_state_is_constructible_without_a_running_app() {
        let state = SystemStatusState::new();
        let defaulted = SystemStatusState::default();

        // The cache is the only field; both constructors must make an independent one.
        assert_eq!(
            std::sync::Arc::strong_count(&state.cache),
            1,
            "new() owns one reference"
        );
        assert_eq!(std::sync::Arc::strong_count(&defaulted.cache), 1);
    }
}
