use serde_json::{Map, Value};

use crate::acp::{
    contract::{RequiredText, SafeText},
    intent::{NormalizedPayload, SafeResourceLocation},
    lifecycle::ItemLifecycle,
    limits::LIMITS,
    runtime::host_browse::browse_tool_summary,
};

use super::{
    AdapterEvent, ClaudeTranslator, MAX_EVENTS_PER_FRAME, MAX_TRACKED_TOOLS, ResourceMapper,
    TranslationError, category_label, lifecycle_for_tool, line_count, parse_tool_state, string,
    tool_category, transition_tool,
};

impl ClaudeTranslator {
    pub(super) fn tool_call(
        &mut self,
        update: &Map<String, Value>,
    ) -> Result<AdapterEvent, TranslationError> {
        let id = RequiredText::new("toolCallId", string(update, "toolCallId")?)?;
        let category = tool_category(update.get("kind").and_then(Value::as_str));
        let state = parse_tool_state(
            update.get("status"),
            crate::acp::lifecycle::ToolState::Pending,
        )?;
        if let Some(current) = self.tools.get(id.as_str()).copied() {
            if current == state {
                return Ok(AdapterEvent::IgnoredDuplicate);
            }
            return Err(TranslationError::DuplicateTool);
        }
        if self.tools.len() >= MAX_TRACKED_TOOLS
            || self.tracked_item_count() >= LIMITS.wire.items_per_turn as usize
        {
            return Err(TranslationError::ToolLimit);
        }
        self.tools.insert(id.as_str().to_owned(), state);
        Ok(AdapterEvent::Item {
            external_key: id,
            lifecycle: lifecycle_for_tool(state),
            tool_state: Some(state),
            payload: NormalizedPayload::ToolCall {
                category,
                label: SafeText::new("toolLabel", category_label(category))?,
                input_summary: browse_tool_summary("toolInput", update),
            },
        })
    }

    pub(super) fn tool_update(
        &mut self,
        update: &Map<String, Value>,
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let id = RequiredText::new("toolCallId", string(update, "toolCallId")?)?;
        let current = self
            .tools
            .get(id.as_str())
            .copied()
            .ok_or(TranslationError::OrphanToolUpdate)?;
        let next = parse_tool_state(update.get("status"), current)?;
        if next == current {
            return Ok(vec![AdapterEvent::IgnoredDuplicate]);
        }
        transition_tool(current, next)?;
        self.tools.insert(id.as_str().to_owned(), next);
        let category = tool_category(update.get("kind").and_then(Value::as_str));
        let mut events = vec![AdapterEvent::Item {
            external_key: id.clone(),
            lifecycle: lifecycle_for_tool(next),
            tool_state: Some(next),
            payload: NormalizedPayload::ToolUpdateResult {
                category,
                label: SafeText::new("toolLabel", category_label(category))?,
                output_summary: browse_tool_summary("toolOutput", update),
            },
        }];
        if let Some(content) = update.get("content").and_then(Value::as_array) {
            if content.len() >= MAX_EVENTS_PER_FRAME {
                return Err(TranslationError::EventLimit);
            }
            for (index, entry) in content.iter().enumerate() {
                if entry.get("type").and_then(Value::as_str) == Some("diff") {
                    let path = entry
                        .get("path")
                        .and_then(Value::as_str)
                        .ok_or(TranslationError::MalformedFrame)?;
                    let resource = match resources.map_absolute(path) {
                        Ok(resource) => SafeResourceLocation::Resource { resource },
                        Err(reason) => SafeResourceLocation::unavailable(reason),
                    };
                    let old_text = entry.get("oldText").and_then(Value::as_str).unwrap_or("");
                    let new_text = entry.get("newText").and_then(Value::as_str).unwrap_or("");
                    events.push(AdapterEvent::Item {
                        external_key: RequiredText::new(
                            "externalItemId",
                            format!("{}:diff:{index}", id.as_str()),
                        )?,
                        lifecycle: ItemLifecycle::Completed,
                        tool_state: None,
                        payload: NormalizedPayload::FileChangeDiff {
                            resource,
                            summary: SafeText::new(
                                "fileSummary",
                                "File change reported by Claude ACP",
                            )?,
                            additions: Some(line_count(new_text)),
                            deletions: Some(line_count(old_text)),
                        },
                    });
                }
            }
        }
        Ok(events)
    }
}
