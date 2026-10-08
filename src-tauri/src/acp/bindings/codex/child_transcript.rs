use crate::acp::{
    bindings::{adapter::ResourceMapper, codex::normalize_transcript_item},
    contract::SafeText,
    intent::NormalizedPayload,
    lifecycle::{ItemLifecycle, ToolState},
};
use serde::{Deserialize, Serialize};
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
pub(crate) const MAX_SNAPSHOT_BYTES: usize = 4_194_304;
pub(crate) const MAX_TURNS: usize = 128;
pub(crate) const MAX_ITEMS: usize = 1024;
const MAX_TEXT_BYTES: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChildTranscriptSnapshot {
    pub(crate) child_thread_id: String,
    pub(crate) parent_thread_id: String,
    pub(crate) turns: Vec<ChildTurn>,
    #[serde(default)]
    pub(crate) retention_limited: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChildTurn {
    pub(crate) id: String,
    pub(crate) status: String,
    pub(crate) items: Vec<ChildItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChildItem {
    pub(crate) id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<ChildTextRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) observed_sequence: Option<u64>,
    pub(crate) lifecycle: ItemLifecycle,
    pub(crate) tool_state: Option<ToolState>,
    pub(crate) payload: NormalizedPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ChildTextRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ChildTranscriptError {
    #[error("child transcript response is too large")]
    Capacity,
    #[error("child transcript response is malformed or incomplete")]
    Malformed,
}

pub(crate) fn normalize_read_response<M: ResourceMapper + ?Sized>(
    wire: &[u8],
    request_id: u64,
    child: &str,
    parent: &str,
    mapper: &M,
) -> Result<ChildTranscriptSnapshot, ChildTranscriptError> {
    if wire.len() > MAX_WIRE_BYTES {
        return Err(ChildTranscriptError::Capacity);
    }
    let value: Value = serde_json::from_slice(wire).map_err(|_| ChildTranscriptError::Malformed)?;
    if value.get("id").and_then(Value::as_u64) != Some(request_id) || value.get("error").is_some() {
        return Err(ChildTranscriptError::Malformed);
    }
    let thread = value
        .get("result")
        .and_then(|result| result.get("thread"))
        .ok_or(ChildTranscriptError::Malformed)?;
    if thread.get("id").and_then(Value::as_str) != Some(child)
        || thread.get("parentThreadId").and_then(Value::as_str) != Some(parent)
    {
        return Err(ChildTranscriptError::Malformed);
    }
    let turns = thread
        .get("turns")
        .and_then(Value::as_array)
        .ok_or(ChildTranscriptError::Malformed)?;
    if turns.len() > MAX_TURNS {
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
                | "exitedReviewMode" => {
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
    let snapshot = ChildTranscriptSnapshot {
        child_thread_id: child.into(),
        parent_thread_id: parent.into(),
        turns: normalized_turns,
        retention_limited: false,
    };
    if serde_json::to_vec(&snapshot)
        .map_err(|_| ChildTranscriptError::Malformed)?
        .len()
        > MAX_SNAPSHOT_BYTES
    {
        return Err(ChildTranscriptError::Capacity);
    }
    Ok(snapshot)
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
    fn full_child_read_normalizes_text_and_rejects_summary_or_foreign_thread() {
        let wire = br#"{"id":9,"result":{"thread":{"id":"child-1","parentThreadId":"parent-1","turns":[{"id":"turn-1","itemsView":"full","status":"completed","items":[{"id":"user-1","type":"userMessage","content":[{"type":"text","text":"Inspect this","text_elements":[]},{"type":"localImage","path":"C:\\secret.png"}]},{"id":"agent-1","type":"agentMessage","text":"Done","phase":null,"memoryCitation":null}]}]}}}"#;
        let mapper = NativeResourceMapper::new([]).unwrap();
        let snapshot = normalize_read_response(wire, 9, "child-1", "parent-1", &mapper).unwrap();
        assert_eq!(snapshot.turns.len(), 1);
        assert_eq!(snapshot.turns[0].items[0].role, Some(ChildTextRole::User));
        assert_eq!(
            snapshot.turns[0].items[1].role,
            Some(ChildTextRole::Assistant)
        );
        assert!(
            matches!(&snapshot.turns[0].items[0].payload, NormalizedPayload::Text { text } if text.as_str() == "Inspect this")
        );
        assert!(
            matches!(&snapshot.turns[0].items[1].payload, NormalizedPayload::Text { text } if text.as_str() == "Done")
        );
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("secret.png")
        );

        let summary = String::from_utf8(wire.to_vec())
            .unwrap()
            .replace("\"full\"", "\"summary\"");
        assert!(
            normalize_read_response(summary.as_bytes(), 9, "child-1", "parent-1", &mapper).is_err()
        );
        let invalid_role = serde_json::from_str::<ChildTextRole>("\"operator\"");
        assert!(invalid_role.is_err());
        assert!(normalize_read_response(wire, 9, "child-2", "parent-1", &mapper).is_err());
        assert!(normalize_read_response(wire, 9, "child-1", "parent-2", &mapper).is_err());
        assert_eq!(
            normalize_read_response(
                &vec![b'x'; MAX_WIRE_BYTES + 1],
                9,
                "child-1",
                "parent-1",
                &mapper
            ),
            Err(ChildTranscriptError::Capacity)
        );
    }

    #[test]
    fn full_child_read_uses_parent_projection_for_command_file_and_reasoning() {
        let mapper = NativeResourceMapper::new([]).unwrap();
        let wire = serde_json::to_vec(&serde_json::json!({
            "id": 23,
            "result": {"thread": {
                "id": "child-1", "parentThreadId": "parent-1",
                "turns": [{
                    "id": "turn-1", "itemsView": "full", "status": "completed",
                    "items": [
                        {"id":"reason-1","type":"reasoning","summary":["Need evidence"],"content":[]},
                        {"id":"command-1","type":"commandExecution","command":"rg -n TODO src","cwd":"C:/repo","status":"completed","aggregatedOutput":"src/a.ts:1:TODO","exitCode":0,"commandActions":[]},
                        {"id":"file-1","type":"fileChange","status":"completed","changes":[{"path":"C:/repo/a.ts","kind":{"type":"update"},"diff":"@@"}]},
                        {"id":"mcp-1","type":"mcpToolCall","server":"docs","tool":"lookup","status":"completed","arguments":{"query":"API"},"result":null,"error":null}
                    ]
                }]
            }}
        })).unwrap();
        let snapshot = normalize_read_response(&wire, 23, "child-1", "parent-1", &mapper).unwrap();
        let items = &snapshot.turns[0].items;
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
        let wire = br#"{"id":3,"result":{"thread":{"id":"child-1","parentThreadId":"parent-1","turns":[{"id":"turn-1","itemsView":"full","status":"completed","items":[{"id":"item-1","type":"newNativeItem","privateToken":"never-persist-this"}]}]}}}"#;
        let snapshot = normalize_read_response(wire, 3, "child-1", "parent-1", &mapper).unwrap();
        assert!(matches!(
            snapshot.turns[0].items[0].payload,
            NormalizedPayload::Unsupported { .. }
        ));
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("never-persist-this")
        );
    }
}
