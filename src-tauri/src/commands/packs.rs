//! Pack catalog and local-install commands.
//!
//! **Ownership.** Thin translation only: validate the request, call one service method on
//! a blocking worker, map the result. No filesystem, SQL, or TOML logic lives here
//! (ARCHITECTURE.md §13, AGENTS.md §5).

use std::sync::{Arc, OnceLock};

use morphix_core::MorphixService;
use morphix_core::packs::catalog::{PackDetail, PackSummary};
use morphix_core::platform::paths::MorphixPaths;
use tauri::State;

use crate::error::IpcError;

/// Lazily constructed pack service.
///
/// Construction can fail (for example, an unusable data directory). The failure is
/// reported by the first command rather than aborting the whole window, so browsing still
/// shows an actionable error.
pub struct PackState {
    service: OnceLock<Arc<MorphixService>>,
    construction_error: OnceLock<String>,
}

impl PackState {
    /// Creates an empty state that opens the service on first use.
    #[must_use]
    pub fn new() -> Self {
        Self {
            service: OnceLock::new(),
            construction_error: OnceLock::new(),
        }
    }

    /// Creates a state around an existing service (tests and embedding).
    #[must_use]
    pub fn with_service(service: Arc<MorphixService>) -> Self {
        let state = Self::new();
        let _ = state.service.set(service);
        state
    }

    fn service(&self) -> Result<Arc<MorphixService>, IpcError> {
        if let Some(service) = self.service.get() {
            return Ok(Arc::clone(service));
        }
        if let Some(error) = self.construction_error.get() {
            return Err(IpcError {
                code: "E_INTERNAL".to_owned(),
                message: error.clone(),
                detail: None,
            });
        }
        let opened =
            MorphixPaths::from_env().and_then(|paths| MorphixService::open(paths).map(Arc::new));
        match opened {
            Ok(service) => {
                let service = self.service.get_or_init(|| service);
                Ok(Arc::clone(service))
            }
            Err(error) => {
                let error = IpcError::from(error);
                let _ = self.construction_error.set(error.message.clone());
                Err(error)
            }
        }
    }
}

impl Default for PackState {
    fn default() -> Self {
        Self::new()
    }
}

/// Lists bundled, installed, and invalid packs.
///
/// # Errors
///
/// Returns `E_INTERNAL` when the service cannot be constructed, or a typed core error when
/// the pack index or bundled root cannot be read.
#[tauri::command]
pub async fn list_packs(state: State<'_, PackState>) -> Result<Vec<PackSummary>, IpcError> {
    let service = state.service()?;
    tauri::async_runtime::spawn_blocking(move || service.list_packs())
        .await
        .map_err(|error| IpcError::internal("list_packs", error.to_string()))?
        .map_err(IpcError::from)
}

/// Returns one pack's detail, including its declared change set.
///
/// # Errors
///
/// Returns `E_INVALID_ID` for an unknown pack and typed core errors for unreadable data.
#[tauri::command]
pub async fn get_pack_detail(
    pack_id: String,
    state: State<'_, PackState>,
) -> Result<PackDetail, IpcError> {
    let service = state.service()?;
    tauri::async_runtime::spawn_blocking(move || service.get_pack_detail(&pack_id))
        .await
        .map_err(|error| IpcError::internal("get_pack_detail", error.to_string()))?
        .map_err(IpcError::from)
}

/// Validates and installs a pack from a directory chosen through the OS dialog.
///
/// This is the only pack command that accepts a path, and it never writes desktop state.
///
/// # Errors
///
/// Returns the validation error with all issues in `detail`; nothing is installed.
#[tauri::command]
pub async fn install_local_pack(
    directory: String,
    state: State<'_, PackState>,
) -> Result<PackSummary, IpcError> {
    let service = state.service()?;
    tauri::async_runtime::spawn_blocking(move || {
        service.install_local_pack(std::path::Path::new(&directory))
    })
    .await
    .map_err(|error| IpcError::internal("install_local_pack", error.to_string()))?
    .map_err(IpcError::from)
}
