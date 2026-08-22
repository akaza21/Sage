use serde::Deserialize;
use tauri::State;

use crate::{
    domain::{FileDiff, RepositoryRef, ResourceRef, RootRepositoryStatus},
    error::AppError,
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct NestedRepositoryRequest {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
    pub r#ref: RepositoryRef,
    pub resource: Option<ResourceRef>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NestedRepositoryFileDiffResult {
    pub project_id: String,
    pub transition_epoch: u64,
    pub diff: FileDiff,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NestedRepositoryStatusResult {
    pub project_id: String,
    pub transition_epoch: u64,
    pub r#ref: RepositoryRef,
    pub status: RootRepositoryStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct DiscoverProjectRepositoriesRequest {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverProjectRepositoriesResult {
    pub project_id: String,
    pub transition_epoch: u64,
    pub refs: Vec<RepositoryRef>,
    pub truncated: bool,
}

#[tauri::command]
pub fn discover_project_repositories(
    _state: State<'_, AppState>,
    request: DiscoverProjectRepositoriesRequest,
) -> Result<DiscoverProjectRepositoriesResult, String> {
    Ok(DiscoverProjectRepositoriesResult {
        project_id: request.project_id,
        transition_epoch: request.expected_transition_epoch,
        refs: Vec::new(),
        truncated: false,
    })
}

#[tauri::command]
pub fn get_repository_ref_status(
    _state: State<'_, AppState>,
    request: NestedRepositoryRequest,
) -> Result<NestedRepositoryStatusResult, String> {
    let _ = request;
    Err(AppError::InvalidRequest("nested repository status is not implemented".into()).to_string())
}

#[tauri::command]
pub fn get_repository_ref_file_diff(
    _state: State<'_, AppState>,
    request: NestedRepositoryRequest,
) -> Result<NestedRepositoryFileDiffResult, String> {
    let _ = request;
    Err(AppError::InvalidRequest("nested repository Diff is not implemented".into()).to_string())
}
