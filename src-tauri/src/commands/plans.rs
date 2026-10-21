//! Plan commands: create and discard change plans (P3-11).
//!
//! **Ownership.** Thin translation only: validate the request, call one service method
//! on a blocking worker, map the result. No filesystem, SQL, or TOML logic lives here
//! (ARCHITECTURE.md 13, AGENTS.md 5).
//!
//! Until Phase 6 supplies the adapter reads, `create_pack_plan` answers
//! `E_ADAPTER_UNAVAILABLE` from the core rather than returning a plan built on guessed
//! state (ADR-024). The command surface is stable; only the refusal is removed.

use morphix_core::MorphixService;
use morphix_core::planning::plan::ChangePlan;
use tauri::State;

use super::packs::PackState;
use crate::error::IpcError;

/// Creates a change plan for an installed pack.
///
/// # Errors
///
/// Returns `E_ADAPTER_UNAVAILABLE` until the Phase 6 adapter reads land, `E_INVALID_ID`
/// for an unknown pack, and `E_INTERNAL` when the service cannot be constructed.
#[tauri::command]
pub async fn create_pack_plan(
    pack_id: String,
    state: State<'_, PackState>,
) -> Result<ChangePlan, IpcError> {
    let service = pack_service(state)?;
    tauri::async_runtime::spawn_blocking(move || service.create_pack_plan(&pack_id))
        .await
        .map_err(|error| IpcError::internal("create_pack_plan", error.to_string()))?
        .map_err(IpcError::from)
}

/// Builds a plan restoring a snapshot.
///
/// # Errors
///
/// Always returns `E_UNSUPPORTED_RESOURCE` in Phase 3: the snapshot engine lands in
/// Phase 4.
#[tauri::command]
pub async fn create_snapshot_restore_plan(
    snapshot_id: String,
    state: State<'_, PackState>,
) -> Result<ChangePlan, IpcError> {
    let service = pack_service(state)?;
    tauri::async_runtime::spawn_blocking(move || service.create_snapshot_restore_plan(&snapshot_id))
        .await
        .map_err(|error| IpcError::internal("create_snapshot_restore_plan", error.to_string()))?
        .map_err(IpcError::from)
}

/// Discards a persisted plan so it can never apply.
///
/// Discard is idempotent.
///
/// # Errors
///
/// Returns a typed core error when the plan store cannot be updated, or `E_INTERNAL`
/// when the service cannot be constructed.
#[tauri::command]
pub async fn discard_plan(plan_id: String, state: State<'_, PackState>) -> Result<(), IpcError> {
    let service = pack_service(state)?;
    tauri::async_runtime::spawn_blocking(move || service.discard_plan(&plan_id))
        .await
        .map_err(|error| IpcError::internal("discard_plan", error.to_string()))?
        .map_err(IpcError::from)
}

/// Shares the pack-backed service, including its construction-error path.
fn pack_service(state: State<'_, PackState>) -> Result<std::sync::Arc<MorphixService>, IpcError> {
    state.core_service()
}
