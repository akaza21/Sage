use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
    domain::ResourceRef,
    error::{AppError, AppResult},
    workspace::Workspace,
};

pub const MAX_QUERY_BYTES: usize = 512;
pub const MAX_REQUEST_ID_BYTES: usize = 128;
pub const MAX_PROJECT_ID_BYTES: usize = 512;
pub const MAX_PROJECT_ROOTS: usize = 8;
pub const MAX_SEARCH_RESULTS: usize = 500;
pub const MAX_SEARCH_FILES: u64 = 20_000;
pub const MAX_SEARCH_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_SEARCH_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_SEARCH_ENTRIES: u64 = 50_000;
pub const MAX_SEARCH_DEPTH: usize = 32;
pub const MAX_SEARCH_PREVIEW_CHARS: usize = 240;
pub const MAX_RESOURCE_PATH_BYTES: usize = 16 * 1024;
pub const MAX_SEARCH_DURATION: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceSearchRequest {
    pub request_id: String,
    pub project_id: String,
    pub expected_transition_epoch: u64,
    pub query: String,
    #[serde(default)]
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSearchMatch {
    pub resource: ResourceRef,
    pub line_number: u32,
    pub column_number: u32,
    pub preview: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchRootState {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchLimitReason {
    #[serde(rename = "resultLimit")]
    Results,
    #[serde(rename = "fileLimit")]
    FileCount,
    #[serde(rename = "fileSizeLimit")]
    FileSize,
    #[serde(rename = "byteLimit")]
    Bytes,
    #[serde(rename = "entryLimit")]
    Entries,
    #[serde(rename = "depthLimit")]
    Depth,
    #[serde(rename = "timeLimit")]
    Time,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRootReport {
    pub root_id: String,
    pub root_name: String,
    pub state: SearchRootState,
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub entries_visited: u64,
    pub skipped_files: u64,
    pub error_count: u64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSearchResponse {
    pub request_id: String,
    pub project_id: String,
    pub transition_epoch: u64,
    pub query: String,
    pub matches: Vec<WorkspaceSearchMatch>,
    pub roots: Vec<SearchRootReport>,
    pub truncated: bool,
    pub limit_reasons: Vec<SearchLimitReason>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchLimits {
    pub max_results: usize,
    pub max_files: u64,
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
    pub max_entries: u64,
    pub max_depth: usize,
    pub max_preview_chars: usize,
    pub max_duration: Duration,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_results: MAX_SEARCH_RESULTS,
            max_files: MAX_SEARCH_FILES,
            max_total_bytes: MAX_SEARCH_TOTAL_BYTES,
            max_file_bytes: MAX_SEARCH_FILE_BYTES,
            max_entries: MAX_SEARCH_ENTRIES,
            max_depth: MAX_SEARCH_DEPTH,
            max_preview_chars: MAX_SEARCH_PREVIEW_CHARS,
            max_duration: MAX_SEARCH_DURATION,
        }
    }
}

pub fn validate_search_request(request: &WorkspaceSearchRequest) -> AppResult<()> {
    if !valid_request_id(&request.request_id) {
        return Err(AppError::InvalidRequest(
            "workspace search request ID is invalid".into(),
        ));
    }
    if request.project_id.is_empty()
        || request.project_id.len() > MAX_PROJECT_ID_BYTES
        || request.project_id.contains('\0')
    {
        return Err(AppError::InvalidRequest(
            "workspace search project ID is invalid".into(),
        ));
    }
    let query = request.query.trim();
    if query.is_empty() {
        return Err(AppError::InvalidRequest(
            "workspace search query must not be empty".into(),
        ));
    }
    if query.len() > MAX_QUERY_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "workspace search query exceeds the {MAX_QUERY_BYTES}-byte limit"
        )));
    }
    Ok(())
}

pub fn valid_request_id(request_id: &str) -> bool {
    !request_id.is_empty()
        && request_id.len() <= MAX_REQUEST_ID_BYTES
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
}

#[derive(Debug)]
pub enum SearchRootSource {
    Available(Workspace),
    Unavailable(String),
}

#[derive(Debug)]
pub struct SearchRoot {
    pub root_id: String,
    pub root_name: String,
    pub source: SearchRootSource,
}
