#[path = "app_preview/mod.rs"]
mod app_preview;

use tauri::State;

use crate::state::AppState;

use self::app_preview::{
    PreviewDiscoveryRequest, PreviewDiscoveryResponse, PreviewError, PreviewHealthResponse,
    PreviewStartResponse, PreviewStopResponse, PreviewTargetRef,
    discovery::{discover, validate_discovery_request, validate_target},
    lifecycle,
};

#[tauri::command]
pub async fn discover_preview_targets(
    state: State<'_, AppState>,
    request: PreviewDiscoveryRequest,
) -> Result<PreviewDiscoveryResponse, PreviewError> {
    discover_preview_targets_inner(&state, request).await
}

#[tauri::command]
pub async fn get_preview_health(
    state: State<'_, AppState>,
    request: PreviewTargetRef,
) -> Result<PreviewHealthResponse, PreviewError> {
    let _epoch = validate_current_target(&state, &request).await?;
    let origin = lifecycle::health(&request);
    let healthy = origin.is_some();
    Ok(PreviewHealthResponse {
        ref_: request,
        origin,
        healthy,
    })
}

#[tauri::command]
pub async fn start_preview_target(
    state: State<'_, AppState>,
    request: PreviewTargetRef,
) -> Result<PreviewStartResponse, PreviewError> {
    let epoch = validate_current_target(&state, &request).await?;
    let start = lifecycle::reserve_start(request)?;
    drop(epoch);
    lifecycle::finish_start(start)
}

#[tauri::command]
pub async fn stop_preview_target(
    state: State<'_, AppState>,
    request: PreviewTargetRef,
) -> Result<PreviewStopResponse, PreviewError> {
    let _epoch = validate_current_target(&state, &request).await?;
    lifecycle::stop(&request)
}

pub(crate) fn stop_preview_children_before_epoch(new_epoch: u64) {
    lifecycle::stop_before_epoch(new_epoch);
}

async fn discover_preview_targets_inner(
    state: &AppState,
    request: PreviewDiscoveryRequest,
) -> Result<PreviewDiscoveryResponse, PreviewError> {
    validate_discovery_request(&request)?;
    let epoch = state.project_transitions.lock().await;
    if *epoch != request.expected_transition_epoch {
        return Err(PreviewError::conflict(
            "transitionEpoch",
            "Preview request belongs to a stale Project transition.",
        ));
    }
    if state
        .database
        .active_project_id()
        .map_err(|_| PreviewError::internal("Preview could not resolve the active Project."))?
        .as_deref()
        != Some(&request.project_id)
    {
        return Err(PreviewError::conflict(
            "project",
            "Preview request does not belong to the active Project.",
        ));
    }
    let roots = state
        .database
        .root_records(&request.project_id)
        .map_err(|_| PreviewError::internal("Preview could not resolve Project roots."))?;
    discover(&request, roots)
}

async fn validate_current_target<'state>(
    state: &'state AppState,
    target: &PreviewTargetRef,
) -> Result<tokio::sync::MutexGuard<'state, u64>, PreviewError> {
    validate_target(target)?;
    let epoch = state.project_transitions.lock().await;
    if *epoch != target.expected_transition_epoch {
        return Err(PreviewError::conflict(
            "transitionEpoch",
            "Preview request belongs to a stale Project transition.",
        ));
    }
    if state
        .database
        .active_project_id()
        .map_err(|_| PreviewError::internal("Preview could not resolve the active Project."))?
        .as_deref()
        != Some(&target.project_id)
    {
        return Err(PreviewError::conflict(
            "project",
            "Preview request does not belong to the active Project.",
        ));
    }
    Ok(epoch)
}
