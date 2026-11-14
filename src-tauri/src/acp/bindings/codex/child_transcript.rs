use crate::acp::{
    bindings::{adapter::ResourceMapper, codex::normalize_transcript_item},
    contract::SafeText,
    intent::NormalizedPayload,
    lifecycle::ItemLifecycle,
};
use crate::acp_protocol::codex_protocol::{ChildItem, ChildTextRole, ChildTurn, MAX_ITEMS};
use serde::Serialize;
use serde_json::Value;

struct TranscriptMapper<'a, M: ?Sized>(&'a M);

impl<M: ResourceMapper + ?Sized> crate::acp::bindings::codex::ResourceMapper
    for TranscriptMapper<'_, M>
{
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>
    {
        self.0.map_existing(absolute_path)
    }

    fn map_missing_display(
        &self,
        absolute_path: &str,
    ) -> Option<crate::acp::intent::SafeResourceRef> {
        self.0.map_missing_display(absolute_path)
    }
}

const MAX_WIRE_BYTES: usize = 1_048_576;
const MAX_TEXT_BYTES: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ChildTranscriptError {
    #[error("child transcript read was rejected")]
    Rejected,
    #[error("child transcript response is too large")]
    Capacity,
    #[error("child transcript response is malformed or incomplete")]
    Malformed,
}

/// One `thread/turns/list` page, in the order it was requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeTurnsPage {
    pub(crate) turns: Vec<ChildTurn>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeSessionSummary {
    pub(crate) thread_id: String,
    pub(crate) title: String,
    /// Unix seconds of the session's latest activity.
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeSessionsPage {
    pub(crate) sessions: Vec<NativeSessionSummary>,
    pub(crate) next_cursor: Option<String>,
}

/// Turns asked for per `thread/turns/list` page. Small pages keep one long
/// Turn from making a page too large; 32 pages still hold `MAX_TURNS`.
pub(crate) const TURN_PAGE: usize = 4;
pub(crate) const SESSION_PAGE: usize = 25;
const MAX_CURSOR_BYTES: usize = 1024;
const MAX_TITLE_CHARS: usize = 200;

fn page_result(wire: &[u8], request_id: u64) -> Result<Value, ChildTranscriptError> {
    if wire.len() > MAX_WIRE_BYTES {
        return Err(ChildTranscriptError::Capacity);
    }
    let mut value: Value =
        serde_json::from_slice(wire).map_err(|_| ChildTranscriptError::Malformed)?;
    if value.get("id").and_then(Value::as_u64) != Some(request_id) || value.get("method").is_some()
    {
        return Err(ChildTranscriptError::Malformed);
    }
    if let Some(error) = value.get("error") {
        return Err(
            if value.get("result").is_none()
                && error.get("code").and_then(Value::as_i64).is_some()
                && error.get("message").and_then(Value::as_str).is_some()
            {
                ChildTranscriptError::Rejected
            } else {
                ChildTranscriptError::Malformed
            },
        );
    }
    value
        .get_mut("result")
        .map(Value::take)
        .ok_or(ChildTranscriptError::Malformed)
}

fn page_cursor(result: &Value) -> Result<Option<String>, ChildTranscriptError> {
    match result.get("nextCursor") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(cursor))
            if !cursor.is_empty()
                && cursor.len() <= MAX_CURSOR_BYTES
                && !cursor.chars().any(char::is_control) =>
        {
            Ok(Some(cursor.clone()))
        }
        Some(_) => Err(ChildTranscriptError::Malformed),
    }
}

pub(crate) fn normalize_sessions_page(
    wire: &[u8],
    request_id: u64,
) -> Result<NativeSessionsPage, ChildTranscriptError> {
    let result = page_result(wire, request_id)?;
    let threads = result
        .get("data")
        .and_then(Value::as_array)
        .filter(|threads| threads.len() <= SESSION_PAGE)
        .ok_or(ChildTranscriptError::Malformed)?;
    let mut sessions = Vec::with_capacity(threads.len());
    for thread in threads {
        // Subagent and ephemeral threads are not sessions a user resumes.
        if thread.get("ephemeral").and_then(Value::as_bool) != Some(false)
            || !thread.get("parentThreadId").is_none_or(Value::is_null)
        {
            continue;
        }
        let title = [thread.get("name"), thread.get("preview")]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
            .find(|text| !text.is_empty())
            .unwrap_or_else(|| "Untitled session".into());
        sessions.push(NativeSessionSummary {
            thread_id: bounded_id(thread.get("id"))?,
            title: title.chars().take(MAX_TITLE_CHARS).collect(),
            updated_at: thread
                .get("updatedAt")
                .and_then(Value::as_i64)
                .ok_or(ChildTranscriptError::Malformed)?,
        });
    }
    Ok(NativeSessionsPage {
        sessions,
        next_cursor: page_cursor(&result)?,
    })
}

pub(crate) fn normalize_turns_page<M: ResourceMapper + ?Sized>(
    wire: &[u8],
    request_id: u64,
    mapper: &M,
) -> Result<NativeTurnsPage, ChildTranscriptError> {
    let result = page_result(wire, request_id)?;
    let turns = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or(ChildTranscriptError::Malformed)?;
    if turns.len() > TURN_PAGE {
        return Err(ChildTranscriptError::Capacity);
    }
    let mut total_items = 0usize;
    let mut normalized_turns = Vec::with_capacity(turns.len());
    for turn in turns {
        if turn.get("itemsView").and_then(Value::as_str) != Some("full") {
            return Err(ChildTranscriptError::Malformed);
        }
        let id = bounded_id(turn.get("id"))?;
        let status = match turn.get("status").and_then(Value::as_str) {
            Some(status @ ("completed" | "interrupted" | "failed" | "inProgress")) => status,
            _ => return Err(ChildTranscriptError::Malformed),
        };
        let items = turn
            .get("items")
            .and_then(Value::as_array)
            .ok_or(ChildTranscriptError::Malformed)?;
        total_items = total_items
            .checked_add(items.len())
            .ok_or(ChildTranscriptError::Capacity)?;
        if total_items > MAX_ITEMS {
            return Err(ChildTranscriptError::Capacity);
        }
        let mut normalized_items = Vec::with_capacity(items.len());
        for item in items {
            let id = bounded_id(item.get("id"))?;
            let lifecycle = if status == "inProgress" {
                ItemLifecycle::Started
            } else {
                ItemLifecycle::Completed
            };
            let kind = item
                .get("type")
                .and_then(Value::as_str)
                .ok_or(ChildTranscriptError::Malformed)?;
            let normalized = match kind {
                "userMessage" => Some((
                    normalize_child_user_message(
                        item.as_object().ok_or(ChildTranscriptError::Malformed)?,
                    )?,
                    None,
                    None,
                )),
                "agentMessage"
                | "plan"
                | "reasoning"
                | "commandExecution"
                | "mcpToolCall"
                | "contextCompaction"
                | "dynamicToolCall"
                | "webSearch"
                | "collabAgentToolCall"
                | "subAgentActivity"
                | "fileChange"
                | "imageGeneration"
                | "imageView"
                | "enteredReviewMode"
                | "exitedReviewMode"
                | "sleep"
                | "functionCallOutput"
                | "hookPrompt" => {
                    let object = item.as_object().ok_or(ChildTranscriptError::Malformed)?;
                    normalize_transcript_item(object, lifecycle, &TranscriptMapper(mapper))
                        .map_err(|_| ChildTranscriptError::Malformed)?
                }
                _ => Some((
                    NormalizedPayload::Unsupported {
                        external_kind: SafeText::new("externalKind", kind)
                            .map_err(|_| ChildTranscriptError::Malformed)?,
                        summary: SafeText::new("summary", "Native item is not projected")
                            .map_err(|_| ChildTranscriptError::Malformed)?,
                    },
                    None,
                    Some(ItemLifecycle::Unsupported),
                )),
            };
            if let Some((payload, tool_state, terminal)) = normalized {
                normalized_items.push(ChildItem {
                    id,
                    role: match kind {
                        "userMessage" => Some(ChildTextRole::User),
                        "agentMessage" => Some(ChildTextRole::Assistant),
                        _ => None,
                    },
                    observed_sequence: None,
                    lifecycle: terminal.unwrap_or(lifecycle),
                    tool_state,
                    payload,
                });
            }
        }
        normalized_turns.push(ChildTurn {
            id,
            status: status.into(),
            items: normalized_items,
        });
    }
    Ok(NativeTurnsPage {
        turns: normalized_turns,
        next_cursor: page_cursor(&result)?,
    })
}

fn bounded_id(value: Option<&Value>) -> Result<String, ChildTranscriptError> {
    let id = value
        .and_then(Value::as_str)
        .ok_or(ChildTranscriptError::Malformed)?;
    if id.is_empty() || id.len() > 512 || id.chars().any(char::is_control) {
        return Err(ChildTranscriptError::Malformed);
    }
    Ok(id.into())
}

fn bounded_text(value: Option<&Value>) -> Result<String, ChildTranscriptError> {
    let text = value
        .and_then(Value::as_str)
        .ok_or(ChildTranscriptError::Malformed)?;
    if text.len() > MAX_TEXT_BYTES {
        return Err(ChildTranscriptError::Capacity);
    }
    Ok(text.into())
}

pub(crate) fn normalize_child_user_message(
    item: &serde_json::Map<String, Value>,
) -> Result<NormalizedPayload, ChildTranscriptError> {
    let text = user_text(item)?;
    Ok(NormalizedPayload::Text {
        text: SafeText::new("text", text).map_err(|_| ChildTranscriptError::Malformed)?,
    })
}

fn user_text(item: &serde_json::Map<String, Value>) -> Result<String, ChildTranscriptError> {
    let content = item
        .get("content")
        .and_then(Value::as_array)
        .ok_or(ChildTranscriptError::Malformed)?;
    if content.len() > 64 {
        return Err(ChildTranscriptError::Capacity);
    }
    let mut text = String::new();
    for part in content {
        if part.get("type").and_then(Value::as_str) == Some("text") {
            let value = bounded_text(part.get("text"))?;
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&value);
            if text.len() > MAX_TEXT_BYTES {
                return Err(ChildTranscriptError::Capacity);
            }
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::resource_mapper::NativeResourceMapper;

    #[test]
    fn full_turns_page_normalizes_text_and_rejects_summary_or_oversize() {
        let wire = br#"{"id":9,"result":{"data":[{"id":"turn-1","itemsView":"full","status":"completed","items":[{"id":"user-1","type":"userMessage","content":[{"type":"text","text":"Inspect this","text_elements":[]},{"type":"localImage","path":"C:\\secret.png"}]},{"id":"agent-1","type":"agentMessage","text":"Done","phase":null,"memoryCitation":null}]}],"nextCursor":"page-2"}}"#;
        let mapper = NativeResourceMapper::new([]).unwrap();
        let page = normalize_turns_page(wire, 9, &mapper).unwrap();
        assert_eq!(page.next_cursor.as_deref(), Some("page-2"));
        assert_eq!(page.turns.len(), 1);
        assert_eq!(page.turns[0].items[0].role, Some(ChildTextRole::User));
        assert_eq!(page.turns[0].items[1].role, Some(ChildTextRole::Assistant));
        assert!(
            matches!(&page.turns[0].items[0].payload, NormalizedPayload::Text { text } if text.as_str() == "Inspect this")
        );
        assert!(
            matches!(&page.turns[0].items[1].payload, NormalizedPayload::Text { text } if text.as_str() == "Done")
        );
        assert!(
            !serde_json::to_string(&page.turns)
                .unwrap()
                .contains("secret.png")
        );

        let summary = String::from_utf8(wire.to_vec())
            .unwrap()
            .replace("\"full\"", "\"summary\"");
        assert!(normalize_turns_page(summary.as_bytes(), 9, &mapper).is_err());
        let invalid_role = serde_json::from_str::<ChildTextRole>("\"operator\"");
        assert!(invalid_role.is_err());
        assert!(normalize_turns_page(wire, 10, &mapper).is_err());
        assert_eq!(
            normalize_turns_page(&vec![b'x'; MAX_WIRE_BYTES + 1], 9, &mapper),
            Err(ChildTranscriptError::Capacity)
        );
        let rejected = br#"{"id":9,"error":{"code":-32600,"message":"no rollout"}}"#;
        assert_eq!(
            normalize_turns_page(rejected, 9, &mapper),
            Err(ChildTranscriptError::Rejected)
        );
    }

    #[test]
    fn full_turns_page_uses_parent_projection_for_command_file_and_reasoning() {
        let mapper = NativeResourceMapper::new([]).unwrap();
        let wire = serde_json::to_vec(&serde_json::json!({
            "id": 23,
            "result": {"nextCursor": null, "data": [{
                "id": "turn-1", "itemsView": "full", "status": "completed",
                "items": [
                    {"id":"reason-1","type":"reasoning","summary":["Need evidence"],"content":[]},
                    {"id":"command-1","type":"commandExecution","command":"rg -n TODO src","cwd":"C:/repo","status":"completed","aggregatedOutput":"src/a.ts:1:TODO","exitCode":0,"commandActions":[]},
                    {"id":"file-1","type":"fileChange","status":"completed","changes":[{"path":"C:/repo/a.ts","kind":{"type":"update"},"diff":"@@"}]},
                    {"id":"mcp-1","type":"mcpToolCall","server":"docs","tool":"lookup","status":"completed","arguments":{"query":"API"},"result":null,"error":null}
                ]
            }]}
        })).unwrap();
        let page = normalize_turns_page(&wire, 23, &mapper).unwrap();
        assert_eq!(page.next_cursor, None);
        let items = &page.turns[0].items;
        assert!(matches!(
            items[0].payload,
            NormalizedPayload::ThoughtSummary { .. }
        ));
        assert!(matches!(
            items[1].payload,
            NormalizedPayload::ToolCall { .. }
        ));
        assert!(matches!(
            items[2].payload,
            NormalizedPayload::FileChangeDiff { .. }
        ));
        assert!(matches!(
            items[3].payload,
            NormalizedPayload::ToolCall { .. }
        ));
    }

    #[test]
    fn unknown_child_item_does_not_persist_native_fields() {
        let mapper = NativeResourceMapper::new([]).unwrap();
        let wire = br#"{"id":3,"result":{"data":[{"id":"turn-1","itemsView":"full","status":"completed","items":[{"id":"item-1","type":"newNativeItem","privateToken":"never-persist-this"}]}]}}"#;
        let page = normalize_turns_page(wire, 3, &mapper).unwrap();
        assert!(matches!(
            page.turns[0].items[0].payload,
            NormalizedPayload::Unsupported { .. }
        ));
        assert!(
            !serde_json::to_string(&page.turns)
                .unwrap()
                .contains("never-persist-this")
        );
    }

    #[test]
    fn sessions_page_lists_resumable_threads_by_title_and_activity() {
        let thread = |id: &str, name: serde_json::Value, preview: &str| {
            serde_json::json!({"id": id, "name": name, "preview": preview, "ephemeral": false,
                "parentThreadId": null, "updatedAt": 1_700_000_000, "turns": []})
        };
        let mut subagent = thread("sub", serde_json::Value::Null, "child");
        subagent["parentThreadId"] = "t-1".into();
        let mut ephemeral = thread("eph", serde_json::Value::Null, "scratch");
        ephemeral["ephemeral"] = true.into();
        let wire = serde_json::to_vec(&serde_json::json!({"id": 4, "result": {
            "data": [
                thread("t-1", "Fix the  build".into(), "ignored"),
                thread("t-2", serde_json::Value::Null, "  Add\ntests "),
                thread("t-3", "".into(), ""),
                subagent,
                ephemeral,
            ],
            "nextCursor": "next"
        }}))
        .unwrap();
        let page = normalize_sessions_page(&wire, 4).unwrap();
        let titles: Vec<_> = page
            .sessions
            .iter()
            .map(|session| (session.thread_id.as_str(), session.title.as_str()))
            .collect();
        assert_eq!(
            titles,
            [
                ("t-1", "Fix the build"),
                ("t-2", "Add tests"),
                ("t-3", "Untitled session")
            ]
        );
        assert_eq!(page.sessions[0].updated_at, 1_700_000_000);
        assert_eq!(page.next_cursor.as_deref(), Some("next"));

        let oversized: Vec<_> = (0..=SESSION_PAGE)
            .map(|index| thread(&format!("t-{index}"), serde_json::Value::Null, "p"))
            .collect();
        let wire = serde_json::to_vec(&serde_json::json!({"id": 4, "result": {"data": oversized}}))
            .unwrap();
        assert_eq!(
            normalize_sessions_page(&wire, 4),
            Err(ChildTranscriptError::Malformed)
        );
    }
}
