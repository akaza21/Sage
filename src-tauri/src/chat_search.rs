use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::{AppError, AppResult},
    persistence::Database,
};

pub const MAX_CHAT_SEARCH_QUERY_BYTES: usize = 256;
pub const MAX_CHAT_SEARCH_RESULTS: usize = 64;
pub const MAX_CHAT_SEARCH_SNIPPET_BYTES: usize = 2 * 1024;
pub const MAX_CHAT_SEARCH_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_CHAT_SCAN_ROWS: usize = MAX_CHAT_SEARCH_RESULTS + 1;
const MAX_ITEM_SCAN_ROWS: usize = 4_096;
const MAX_ITEM_SCAN_BYTES: usize = 32 * 1024 * 1024;
const MAX_CHAT_SEARCH_DURATION: Duration = Duration::from_millis(250);
const MAX_SAFE_JAVASCRIPT_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectChatSearchRequest {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectChatSearchResult {
    pub chat_id: String,
    pub title: String,
    pub attached: bool,
    pub matched_title: bool,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectChatSearchResponse {
    pub project_id: String,
    pub project_generation: u64,
    pub transition_epoch: u64,
    pub query: String,
    pub results: Vec<ProjectChatSearchResult>,
    pub has_more: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct SearchLimits {
    max_chat_rows: usize,
    max_item_rows: usize,
    max_item_bytes: usize,
    max_results: usize,
    max_response_bytes: usize,
    deadline: Duration,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_chat_rows: MAX_CHAT_SCAN_ROWS,
            max_item_rows: MAX_ITEM_SCAN_ROWS,
            max_item_bytes: MAX_ITEM_SCAN_BYTES,
            max_results: MAX_CHAT_SEARCH_RESULTS,
            max_response_bytes: MAX_CHAT_SEARCH_RESPONSE_BYTES,
            deadline: MAX_CHAT_SEARCH_DURATION,
        }
    }
}

#[derive(Debug)]
struct ChatRow {
    id: String,
    title: String,
    attached: bool,
    matched_title: bool,
    snippet: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaggedText {
    kind: String,
    payload: TextBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextBody {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyText {
    role: String,
    text: String,
}

pub fn validate_chat_search_request(request: &ProjectChatSearchRequest) -> AppResult<String> {
    if request.project_id.is_empty()
        || request.project_id.len() > 512
        || request.project_id.chars().any(char::is_control)
    {
        return Err(AppError::InvalidRequest(
            "Chat search requires a valid Project identifier".into(),
        ));
    }
    if request.project_generation > MAX_SAFE_JAVASCRIPT_INTEGER
        || request.expected_transition_epoch > MAX_SAFE_JAVASCRIPT_INTEGER
    {
        return Err(AppError::InvalidRequest(
            "Chat search intent generation is invalid".into(),
        ));
    }
    let query = request.query.trim();
    if query.is_empty() {
        return Err(AppError::InvalidRequest(
            "Chat search query must not be empty".into(),
        ));
    }
    if query.len() > MAX_CHAT_SEARCH_QUERY_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "Chat search query exceeds the {MAX_CHAT_SEARCH_QUERY_BYTES}-byte limit"
        )));
    }
    if query.chars().any(char::is_control) {
        return Err(AppError::InvalidRequest(
            "Chat search query must not contain control characters".into(),
        ));
    }
    Ok(query.into())
}

pub(crate) fn search_project_chats(
    database: &Database,
    request: &ProjectChatSearchRequest,
) -> AppResult<ProjectChatSearchResponse> {
    search_with_limits(database, request, SearchLimits::default())
}

fn search_with_limits(
    database: &Database,
    request: &ProjectChatSearchRequest,
    limits: SearchLimits,
) -> AppResult<ProjectChatSearchResponse> {
    let query = validate_chat_search_request(request)?;
    let started = Instant::now();
    let deadline = started + limits.deadline;
    let connection = database.connection();
    connection.progress_handler(1_000, Some(move || Instant::now() >= deadline))?;
    struct ResetProgress<'a>(&'a rusqlite::Connection);
    impl Drop for ResetProgress<'_> {
        fn drop(&mut self) {
            let _ = self.0.progress_handler(0, None::<fn() -> bool>);
        }
    }
    let _reset = ResetProgress(&connection);
    ensure_deadline(started, limits.deadline)?;

    let mut chats = load_chats(&connection, &request.project_id, &query, limits)?;
    preflight_items(&connection, &request.project_id, limits)?;
    load_primary_text_matches(
        &connection,
        &request.project_id,
        &query,
        &mut chats,
        started,
        limits.deadline,
    )?;
    ensure_deadline(started, limits.deadline)?;

    let mut results = Vec::with_capacity(limits.max_results);
    let mut has_more = false;
    for chat in chats {
        if !chat.matched_title && chat.snippet.is_none() {
            continue;
        }
        let result = ProjectChatSearchResult {
            chat_id: chat.id,
            title: chat.title,
            attached: chat.attached,
            matched_title: chat.matched_title,
            snippet: chat.snippet,
        };
        if results.len() == limits.max_results
            || !fits_response_budget(
                request,
                &query,
                &results,
                &result,
                limits.max_response_bytes,
            )?
        {
            has_more = true;
            continue;
        }
        results.push(result);
    }
    let response = ProjectChatSearchResponse {
        project_id: request.project_id.clone(),
        project_generation: request.project_generation,
        transition_epoch: request.expected_transition_epoch,
        query,
        results,
        has_more,
        elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    };
    if response_bytes(&response)? > limits.max_response_bytes {
        return Err(AppError::ResourceLimit(
            "Chat search response exceeds its byte ceiling".into(),
        ));
    }
    Ok(response)
}

fn load_chats(
    connection: &rusqlite::Connection,
    project_id: &str,
    query: &str,
    limits: SearchLimits,
) -> AppResult<Vec<ChatRow>> {
    let limit = i64::try_from(limits.max_chat_rows.saturating_add(1)).unwrap_or(i64::MAX);
    let mut statement = connection.prepare(
        "SELECT c.id,c.title,EXISTS(
           SELECT 1 FROM chat_tabs a WHERE a.project_id=c.project_id AND a.chat_id=c.id
         )
         FROM chats c WHERE c.project_id=?1
         ORDER BY c.updated_at DESC,c.ordinal DESC,c.id ASC LIMIT ?2",
    )?;
    let rows = statement.query_map(params![project_id, limit], |row| {
        let title: String = row.get(1)?;
        Ok(ChatRow {
            id: row.get(0)?,
            matched_title: title.contains(query),
            title,
            attached: row.get(2)?,
            snippet: None,
        })
    })?;
    let chats = rows.collect::<Result<Vec<_>, _>>()?;
    if chats.len() > limits.max_chat_rows {
        return Err(AppError::ResourceLimit(
            "Chat search Chat-row ceiling was exceeded".into(),
        ));
    }
    Ok(chats)
}

fn preflight_items(
    connection: &rusqlite::Connection,
    project_id: &str,
    limits: SearchLimits,
) -> AppResult<()> {
    let limit = i64::try_from(limits.max_item_rows.saturating_add(1)).unwrap_or(i64::MAX);
    let (rows, bytes): (i64, i64) = connection.query_row(
        "SELECT COUNT(*),COALESCE(SUM(payload_bytes),0) FROM (
           SELECT payload_bytes FROM items INDEXED BY idx_items_owner
           WHERE project_id=?1 LIMIT ?2
         )",
        params![project_id, limit],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if rows < 0 || rows as usize > limits.max_item_rows {
        return Err(AppError::ResourceLimit(
            "Chat search item-row ceiling was exceeded".into(),
        ));
    }
    if bytes < 0 || bytes as usize > limits.max_item_bytes {
        return Err(AppError::ResourceLimit(
            "Chat search primary-history byte ceiling was exceeded".into(),
        ));
    }
    Ok(())
}

fn load_primary_text_matches(
    connection: &rusqlite::Connection,
    project_id: &str,
    query: &str,
    chats: &mut [ChatRow],
    started: Instant,
    deadline: Duration,
) -> AppResult<()> {
    let by_id = chats
        .iter()
        .enumerate()
        .map(|(index, chat)| (chat.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut statement = connection.prepare(
        "SELECT i.chat_id,i.payload_json,i.payload_bytes,i.payload_digest
         FROM items i JOIN chats c ON c.project_id=i.project_id AND c.id=i.chat_id
         WHERE i.project_id=?1 AND i.origin='primary' AND i.kind='text'
         ORDER BY c.updated_at DESC,c.ordinal DESC,c.id ASC,
                  i.created_at DESC,i.ordinal DESC,i.id ASC",
    )?;
    let mut rows = statement.query([project_id])?;
    while let Some(row) = map_deadline(rows.next(), started, deadline)? {
        let chat_id: String = row.get(0)?;
        let payload: String = row.get(1)?;
        let payload_bytes: i64 = row.get(2)?;
        let payload_digest: String = row.get(3)?;
        if payload_bytes < 0
            || payload_bytes as usize != payload.len()
            || format!("{:x}", Sha256::digest(payload.as_bytes())) != payload_digest
        {
            return Err(AppError::InvalidRequest(
                "Chat search found corrupt primary text history".into(),
            ));
        }
        let text = decode_primary_text(&payload)?;
        let Some(position) = text.find(query) else {
            continue;
        };
        if let Some(index) = by_id.get(&chat_id).copied()
            && chats[index].snippet.is_none()
        {
            chats[index].snippet = Some(snippet(&text, position));
        }
    }
    Ok(())
}

fn decode_primary_text(payload: &str) -> AppResult<String> {
    if let Ok(tagged) = serde_json::from_str::<TaggedText>(payload)
        && tagged.kind == "text"
    {
        return Ok(tagged.payload.text);
    }
    if let Ok(legacy) = serde_json::from_str::<LegacyText>(payload)
        && matches!(legacy.role.as_str(), "user" | "assistant" | "system")
    {
        return Ok(legacy.text);
    }
    Err(AppError::InvalidRequest(
        "Chat search found an invalid primary text payload".into(),
    ))
}

fn snippet(text: &str, match_start: usize) -> String {
    if text.len() <= MAX_CHAT_SEARCH_SNIPPET_BYTES {
        return text.into();
    }
    let mut start = match_start.saturating_sub(MAX_CHAT_SEARCH_SNIPPET_BYTES / 2);
    while start < match_start && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (start + MAX_CHAT_SEARCH_SNIPPET_BYTES).min(text.len());
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[start..end].into()
}

fn fits_response_budget(
    request: &ProjectChatSearchRequest,
    query: &str,
    accepted: &[ProjectChatSearchResult],
    candidate: &ProjectChatSearchResult,
    maximum: usize,
) -> AppResult<bool> {
    let mut results = accepted.to_vec();
    results.push(candidate.clone());
    let probe = ProjectChatSearchResponse {
        project_id: request.project_id.clone(),
        project_generation: request.project_generation,
        transition_epoch: request.expected_transition_epoch,
        query: query.into(),
        results,
        has_more: true,
        elapsed_ms: u64::MAX,
    };
    Ok(response_bytes(&probe)? <= maximum)
}

fn response_bytes(response: &ProjectChatSearchResponse) -> AppResult<usize> {
    serde_json::to_vec(response)
        .map(|bytes| bytes.len())
        .map_err(|_| AppError::InvalidRequest("Chat search response could not be encoded".into()))
}

fn ensure_deadline(started: Instant, deadline: Duration) -> AppResult<()> {
    if started.elapsed() >= deadline {
        return Err(AppError::ResourceLimit(
            "Chat search deadline was exceeded".into(),
        ));
    }
    Ok(())
}

fn map_deadline<T>(
    result: Result<T, rusqlite::Error>,
    started: Instant,
    deadline: Duration,
) -> AppResult<T> {
    match result {
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::OperationInterrupted =>
        {
            Err(AppError::ResourceLimit(
                "Chat search deadline was exceeded".into(),
            ))
        }
        other => {
            ensure_deadline(started, deadline)?;
            Ok(other?)
        }
    }
}

#[cfg(test)]
mod tests;
