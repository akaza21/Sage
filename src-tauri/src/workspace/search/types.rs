use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{domain::ResourceRef, workspace::Workspace};

pub const MAX_QUERY_BYTES: usize = 512;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSearchRequest {
    pub project_id: String,
    pub query: String,
    #[serde(default)]
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSearchMatch {
    pub resource: ResourceRef,
    pub line_number: u32,
    pub column_number: u32,
    pub preview: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchRootState {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSearchResponse {
    pub project_id: String,
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
            max_results: 500,
            max_files: 20_000,
            max_total_bytes: 64 * 1024 * 1024,
            max_file_bytes: 2 * 1024 * 1024,
            max_entries: 50_000,
            max_depth: 32,
            max_preview_chars: 240,
            max_duration: Duration::from_secs(2),
        }
    }
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
