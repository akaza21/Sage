use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
pub const MAX_CONTINUATION_TOKEN_BYTES: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceSearchRequest {
    pub request_id: String,
    pub project_id: String,
    pub expected_transition_epoch: u64,
    #[serde(default)]
    pub index_generation: u64,
    #[serde(default)]
    pub continuation_token: Option<String>,
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
    pub index_generation: u64,
    pub query: String,
    pub matches: Vec<WorkspaceSearchMatch>,
    pub roots: Vec<SearchRootReport>,
    pub truncated: bool,
    pub continuation_token: Option<String>,
    pub limit_reasons: Vec<SearchLimitReason>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContinuationToken {
    #[serde(rename = "v")]
    version: u8,
    #[serde(rename = "g")]
    index_generation: u64,
    #[serde(rename = "e")]
    transition_epoch: u64,
    #[serde(rename = "c")]
    case_sensitive: u8,
    #[serde(rename = "q")]
    query_hash: String,
    #[serde(rename = "r")]
    root_id: String,
    #[serde(rename = "p")]
    relative_path: String,
    #[serde(rename = "l")]
    line_number: u32,
    #[serde(rename = "o")]
    column_number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationCursor {
    pub root_id: String,
    pub relative_path: String,
    pub line_number: u32,
    pub column_number: u32,
}

fn read_continuation_token(value: &str) -> AppResult<ContinuationToken> {
    if value.len() > MAX_CONTINUATION_TOKEN_BYTES {
        return Err(invalid_continuation());
    }
    let token: ContinuationToken =
        serde_json::from_str(value).map_err(|_| invalid_continuation())?;
    if token.version != 1
        || !matches!(token.case_sensitive, 0 | 1)
        || !valid_sha256(&token.query_hash)
        || !valid_token_identifier(&token.root_id)
        || !valid_relative_path(&token.relative_path)
        || token.line_number == 0
        || token.column_number == 0
    {
        return Err(invalid_continuation());
    }
    Ok(token)
}

pub fn parse_continuation_token(value: &str) -> AppResult<ContinuationCursor> {
    Ok(continuation_cursor(read_continuation_token(value)?))
}

pub fn validate_continuation_stamp(
    value: &str,
    request: &WorkspaceSearchRequest,
    query: &str,
) -> AppResult<ContinuationCursor> {
    let token = read_continuation_token(value)?;
    if token.index_generation != request.index_generation
        || token.transition_epoch != request.expected_transition_epoch
        || token.case_sensitive != u8::from(request.case_sensitive)
        || token.query_hash != search_query_hash(query)
    {
        return Err(stale_continuation());
    }
    Ok(continuation_cursor(token))
}

pub fn encode_continuation_token(
    request: &WorkspaceSearchRequest,
    query: &str,
    cursor: &ContinuationCursor,
) -> Option<String> {
    let encoded = serde_json::to_string(&ContinuationToken {
        version: 1,
        index_generation: request.index_generation,
        transition_epoch: request.expected_transition_epoch,
        case_sensitive: u8::from(request.case_sensitive),
        query_hash: search_query_hash(query),
        root_id: cursor.root_id.clone(),
        relative_path: cursor.relative_path.clone(),
        line_number: cursor.line_number,
        column_number: cursor.column_number,
    })
    .expect("continuation token serialization is infallible");
    (encoded.len() <= MAX_CONTINUATION_TOKEN_BYTES).then_some(encoded)
}

fn continuation_cursor(token: ContinuationToken) -> ContinuationCursor {
    ContinuationCursor {
        root_id: token.root_id,
        relative_path: token.relative_path,
        line_number: token.line_number,
        column_number: token.column_number,
    }
}

pub fn invalid_continuation() -> AppError {
    AppError::InvalidRequest("workspace search continuation is invalid".into())
}

pub fn stale_continuation() -> AppError {
    AppError::InvalidRequest("workspace search continuation is stale".into())
}

fn search_query_hash(query: &str) -> String {
    format!("{:x}", Sha256::digest(query.as_bytes()))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_token_identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_PROJECT_ID_BYTES && !value.contains('\0')
}

fn valid_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RESOURCE_PATH_BYTES
        && !value.contains('\0')
        && !value.contains('\\')
        && !value.starts_with('/')
        && !value.ends_with('/')
        && value
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
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
