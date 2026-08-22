//! Owner-landed App Preview command stubs. IDE-3 replaces the bodies.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // IDE-3 fills the command bodies.
pub struct PreviewDiscoverRequest {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewTargetRef {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
    pub root_id: String,
    pub script_name: String,
}

#[tauri::command]
pub fn discover_preview_targets(
    _request: PreviewDiscoverRequest,
) -> Result<serde_json::Value, String> {
    Err("App Preview lifecycle is not implemented".into())
}

#[tauri::command]
pub fn get_preview_health(_request: PreviewTargetRef) -> Result<serde_json::Value, String> {
    Err("App Preview lifecycle is not implemented".into())
}

#[tauri::command]
pub fn start_preview_target(_request: PreviewTargetRef) -> Result<serde_json::Value, String> {
    Err("App Preview lifecycle is not implemented".into())
}

#[tauri::command]
pub fn stop_preview_target(_request: PreviewTargetRef) -> Result<serde_json::Value, String> {
    Err("App Preview lifecycle is not implemented".into())
}
