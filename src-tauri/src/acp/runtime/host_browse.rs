#![allow(dead_code)] // Curated host-tool dispatch is invoked from tests and ACP adapters.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use url::Url;

use crate::acp::contract::SafeText;
use crate::acp::intent::NormalizedPayload;

pub(crate) const CURATED_APPLY_FILE_TOOL_NAME: &str = "sage.apply_file";
pub(crate) const CURATED_BROWSE_TOOL_NAME: &str = "_sage/browse";
pub(crate) const MAX_HARNESS_BROWSE_URL_BYTES: usize = 2048;
pub(crate) const MAX_HARNESS_BROWSE_SESSIONS: usize = 2;

const MAX_ID_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowseIntent {
    pub chat_id: String,
    pub session_id: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowseSession {
    pub chat_id: String,
    pub session_id: Option<String>,
    pub href: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostBrowseRequest {
    pub session_id: Option<String>,
    pub url: String,
}

#[derive(Debug, Default)]
pub(crate) struct BrowseHost {
    sessions: BTreeMap<String, BrowseSession>,
    next_revision: u64,
}

pub(crate) fn advertised_host_tools() -> &'static [&'static str] {
    &[CURATED_APPLY_FILE_TOOL_NAME, CURATED_BROWSE_TOOL_NAME]
}

pub(crate) fn is_advertised_host_tool(name: &str) -> bool {
    advertised_host_tools().contains(&name)
}

/// The shared ACP conformance shape. Production callers must only use this
/// after installing the owner-bound P-02 read route; deferred bindings test it
/// as wire conformance and do not become live by serializing it.
pub(crate) fn advertised_host_client_capabilities() -> Value {
    json!({
        "fs": { "readTextFile": true, "writeTextFile": false },
        "terminal": false,
        "_meta": { "sage": { "tools": advertised_host_tools() } }
    })
}

pub(crate) fn admit_browse_url(raw: &str) -> Option<String> {
    if raw.is_empty()
        || raw.len() > MAX_HARNESS_BROWSE_URL_BYTES
        || raw.bytes().any(|byte| byte < 32 || byte == 127)
    {
        return None;
    }
    let parsed = Url::parse(raw).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    let port = parsed.port()?;
    let host = match parsed.host_str()? {
        "127.0.0.1" => "127.0.0.1",
        "::1" | "[::1]" => "[::1]",
        _ => return None,
    };
    let path = parsed.path();
    let query = parsed
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let fragment = parsed
        .fragment()
        .map(|fragment| format!("#{fragment}"))
        .unwrap_or_default();
    Some(format!(
        "{}://{host}:{port}{path}{query}{fragment}",
        parsed.scheme()
    ))
}

pub(crate) fn admitted_browse_href_from_tool(update: &Map<String, Value>) -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(url) = update.get("url").and_then(Value::as_str) {
        candidates.push(url);
    }
    if let Some(locations) = update.get("locations").and_then(Value::as_array) {
        for location in locations.iter().take(8) {
            if let Some(url) = location.get("url").and_then(Value::as_str) {
                candidates.push(url);
            }
        }
    }
    if let Some(title) = update
        .get("title")
        .or_else(|| update.get("name"))
        .and_then(Value::as_str)
        && admit_browse_url(title).is_some()
    {
        candidates.insert(0, title);
    }
    candidates.into_iter().find_map(admit_browse_url)
}

pub(crate) fn browse_tool_summary(
    field: &'static str,
    update: &Map<String, Value>,
) -> Option<SafeText<65536>> {
    admitted_browse_href_from_tool(update).and_then(|href| SafeText::new(field, href).ok())
}

pub(crate) fn browse_tool_payload(
    href: &str,
) -> Result<NormalizedPayload, crate::acp::contract::ContractError> {
    Ok(NormalizedPayload::ToolCall {
        category: crate::acp::intent::ToolCategory::Web,
        label: SafeText::new("toolLabel", CURATED_BROWSE_TOOL_NAME)?,
        input_summary: Some(SafeText::new("toolInput", href)?),
        output_summary: None,
        locations: Default::default(),
        command_activity: Default::default(),
    })
}

pub(crate) fn parse_host_browse_request(
    method: &str,
    params: Option<&Map<String, Value>>,
) -> Option<HostBrowseRequest> {
    if method != CURATED_BROWSE_TOOL_NAME {
        return None;
    }
    let params = params?;
    Some(HostBrowseRequest {
        session_id: params
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        url: params
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
    })
}

pub(crate) fn host_browse_session_matches(
    request: &HostBrowseRequest,
    active_session: &str,
) -> bool {
    request
        .session_id
        .as_deref()
        .is_none_or(|session| session == active_session)
}

impl BrowseHost {
    pub(crate) fn dispatch(&mut self, intent: &BrowseIntent) -> Option<BrowseSession> {
        if !is_safe_id(&intent.chat_id) {
            return None;
        }
        if intent
            .session_id
            .as_deref()
            .is_some_and(|session| !is_safe_id(session))
        {
            return None;
        }
        let href = admit_browse_url(&intent.url)?;
        if let Some(existing) = self.sessions.get(&intent.chat_id) {
            if sessions_conflict(existing.session_id.as_deref(), intent.session_id.as_deref()) {
                return None;
            }
            if existing.href == href {
                return Some(existing.clone());
            }
        }
        if !self.sessions.contains_key(&intent.chat_id)
            && self.sessions.len() >= MAX_HARNESS_BROWSE_SESSIONS
        {
            let oldest = self
                .sessions
                .iter()
                .min_by_key(|(_, session)| session.revision)
                .map(|(chat_id, _)| chat_id.clone());
            if let Some(oldest) = oldest {
                self.sessions.remove(&oldest);
            }
        }
        let revision = self.next_revision.max(1);
        self.next_revision = revision.saturating_add(1);
        let session = BrowseSession {
            chat_id: intent.chat_id.clone(),
            session_id: intent.session_id.clone(),
            href,
            revision,
        };
        self.sessions
            .insert(intent.chat_id.clone(), session.clone());
        Some(session)
    }

    pub(crate) fn session_for_chat(&self, chat_id: &str) -> Option<&BrowseSession> {
        self.sessions.get(chat_id)
    }
}

fn is_safe_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_BYTES && !value.contains('\0')
}

fn sessions_conflict(existing: Option<&str>, incoming: Option<&str>) -> bool {
    matches!((existing, incoming), (Some(left), Some(right)) if left != right)
}

#[cfg(test)]
mod tests;
