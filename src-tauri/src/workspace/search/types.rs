use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    domain::ResourceRef,
    error::{AppError, AppResult},
    workspace::Workspace,
    workspace::file_walk::MAX_WORKSPACE_FILES,
    workspace::path_glob::valid_path_glob,
};

pub const MAX_QUERY_BYTES: usize = 512;
pub const MAX_REQUEST_ID_BYTES: usize = 128;
pub const MAX_PROJECT_ID_BYTES: usize = 512;
pub const MAX_PROJECT_ROOTS: usize = 8;
pub const MAX_SEARCH_RESULTS: usize = 500;
/// How many pages of results one Search may show, its first included; the
/// panel's `continuationPages` limit.
pub const MAX_SEARCH_PAGES: usize = 4;
pub const MAX_SEARCH_FILES: u64 = MAX_WORKSPACE_FILES;
pub const MAX_SEARCH_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_SEARCH_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_SEARCH_ENTRIES: u64 = MAX_WORKSPACE_FILES;
pub const MAX_SEARCH_DEPTH: usize = 32;
pub const MAX_SEARCH_PREVIEW_CHARS: usize = 240;
pub const MAX_RESOURCE_PATH_BYTES: usize = 16 * 1024;
pub const MAX_SEARCH_DURATION: Duration = Duration::from_secs(2);
pub const MAX_CONTINUATION_TOKEN_BYTES: usize = 1024;
pub const MAX_SEARCH_FILTER_PATTERNS: usize = 16;
pub const MAX_SEARCH_FILTER_BYTES: usize = 2 * 1024;

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
    #[serde(default)]
    pub whole_word: bool,
    /// The query is a regular expression.
    #[serde(default)]
    pub use_regex: bool,
    #[serde(default)]
    pub include_patterns: Vec<String>,
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
    /// Skip what `.gitignore`, `.ignore` and Git's exclude files ignore
    /// (`search.useIgnoreFiles`).
    #[serde(default = "use_ignore_files_default")]
    pub use_ignore_files: bool,
    /// The `files.exclude` patterns; absent means the registry default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_exclude: Option<Vec<String>>,
}

impl WorkspaceSearchRequest {
    /// The `files.exclude` patterns this search leaves out.
    pub fn files_exclude(&self) -> Vec<String> {
        self.files_exclude
            .clone()
            .unwrap_or_else(crate::settings_store::default_files_exclude)
    }
}

fn use_ignore_files_default() -> bool {
    true
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
    #[serde(rename = "w", default)]
    whole_word: u8,
    /// Written only for a regular-expression search, so other tokens are
    /// as they were.
    #[serde(rename = "x", default, skip_serializing_if = "is_zero")]
    use_regex: u8,
    #[serde(rename = "f")]
    filter_hash: String,
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
        || !matches!(token.whole_word, 0 | 1)
        || !matches!(token.use_regex, 0 | 1)
        || !valid_sha256(&token.filter_hash)
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

pub fn validate_continuation_stamp(
    value: &str,
    request: &WorkspaceSearchRequest,
    query: &str,
) -> AppResult<ContinuationCursor> {
    let token = read_continuation_token(value)?;
    if token.index_generation != request.index_generation
        || token.transition_epoch != request.expected_transition_epoch
        || token.case_sensitive != u8::from(request.case_sensitive)
        || token.whole_word != u8::from(request.whole_word)
        || token.use_regex != u8::from(request.use_regex)
        || token.filter_hash != search_filter_hash(request)
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
        whole_word: u8::from(request.whole_word),
        use_regex: u8::from(request.use_regex),
        filter_hash: search_filter_hash(request),
        query_hash: search_query_hash(query),
        root_id: cursor.root_id.clone(),
        relative_path: cursor.relative_path.clone(),
        line_number: cursor.line_number,
        column_number: cursor.column_number,
    })
    .expect("continuation token serialization is infallible");
    (encoded.len() <= MAX_CONTINUATION_TOKEN_BYTES).then_some(encoded)
}

fn is_zero(value: &u8) -> bool {
    *value == 0
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

fn search_filter_hash(request: &WorkspaceSearchRequest) -> String {
    let mut hasher = Sha256::new();
    for (kind, patterns) in [
        (b'i', &request.include_patterns),
        (b'x', &request.exclude_patterns),
    ] {
        hasher.update([kind]);
        for pattern in patterns {
            hasher.update((pattern.len() as u64).to_be_bytes());
            hasher.update(pattern.as_bytes());
        }
    }
    // Only the non-default choice is hashed, so earlier tokens stay valid.
    if !request.use_ignore_files {
        hasher.update(b"n");
    }
    if let Some(patterns) = &request.files_exclude {
        hasher.update(b"e");
        for pattern in patterns {
            hasher.update((pattern.len() as u64).to_be_bytes());
            hasher.update(pattern.as_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
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
    super::path_filter::validate_patterns(&request.include_patterns, &request.exclude_patterns)?;
    if let Some(patterns) = &request.files_exclude
        && (patterns.len() > crate::settings_store::MAX_GLOBS
            || !patterns.iter().all(|pattern| valid_path_glob(pattern)))
    {
        return Err(AppError::InvalidRequest(
            "workspace search files.exclude patterns are invalid".into(),
        ));
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

pub(super) fn unavailable_report(
    root_id: String,
    root_name: String,
    message: String,
) -> SearchRootReport {
    SearchRootReport {
        root_id,
        root_name,
        state: SearchRootState::Unavailable,
        files_scanned: 0,
        bytes_scanned: 0,
        entries_visited: 0,
        skipped_files: 0,
        error_count: 1,
        message: Some(message),
    }
}

pub(super) fn contract_join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.into()
    } else {
        format!("{parent}/{name}")
    }
}

pub(super) fn push_reason(reasons: &mut Vec<SearchLimitReason>, reason: SearchLimitReason) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}
