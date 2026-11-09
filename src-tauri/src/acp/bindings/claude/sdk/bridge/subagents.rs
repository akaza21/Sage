//! Subagents: a `Task` tool use opens a child transcript, the frames Claude
//! Code tags with that tool use fill it, and the tool's result closes it.

use serde_json::{Value, json};

use super::{Inbound, MAX_TRACKED_TOOLS, SdkBridge, notices::prose};
use crate::acp::bindings::claude::runtime::{ChildEvent, ChildOutcome};
use crate::acp::bindings::claude::sdk::tools::{result_text, text_content, tool_fields};

/// Claude Code's subagent tool, under its current and earlier name.
fn spawns(name: &str) -> bool {
    matches!(name, "Agent" | "Task")
}

impl SdkBridge {
    /// A parent tool use that starts a subagent; false for any other tool.
    pub(super) fn spawn(
        &mut self,
        id: &str,
        name: &str,
        input: &Value,
        inbound: &mut Inbound,
    ) -> bool {
        if !spawns(name) || self.subagents.len() >= MAX_TRACKED_TOOLS {
            return false;
        }
        let text = |key: &str, max| {
            input
                .get(key)
                .and_then(Value::as_str)
                .and_then(|text| prose(text, max))
        };
        self.subagents.insert(id.to_owned());
        inbound.children.push(ChildEvent::Spawned {
            key: id.to_owned(),
            label: text("description", 512)
                .or_else(|| text("subagent_type", 512))
                .unwrap_or_default(),
            prompt: text("prompt", 16_384).unwrap_or_default(),
            model: text("model", 256),
        });
        true
    }

    /// A frame from inside a subagent: its finished messages and tool
    /// results. Its streamed deltas are left out; the finished message
    /// carries the same text.
    pub(super) fn nested(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let Some(key) = sdk.get("parent_tool_use_id").and_then(Value::as_str) else {
            return;
        };
        if !self.subagents.contains(key) {
            return;
        }
        let message = &sdk["message"];
        let blocks = message
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        match sdk.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                let message_id = message
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("message");
                for (index, block) in blocks.enumerate() {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            let Some(text) = block.get("text").and_then(Value::as_str) else {
                                continue;
                            };
                            self.child(
                                inbound,
                                key,
                                json!({
                                    "sessionUpdate": "agent_message_chunk",
                                    "itemId": format!("{message_id}:{index}"),
                                    "content": {"type": "text", "text": text},
                                }),
                            );
                        }
                        Some("tool_use") => self.child_tool(key, block, inbound),
                        _ => {}
                    }
                }
            }
            Some("user") => {
                for block in blocks {
                    let Some(id) = block.get("tool_use_id").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some((_, name)) = self.child_tools.remove(id) else {
                        continue;
                    };
                    let failed = block.get("is_error").and_then(Value::as_bool) == Some(true);
                    let mut update = json!({
                        "sessionUpdate": "tool_call_update",
                        "toolCallId": id,
                        "status": if failed { "failed" } else { "completed" },
                        "_meta": {"claudeCode": {"toolName": name}},
                    });
                    if let Some(text) = result_text(block.get("content")) {
                        update["content"] = json!([text_content(&text)]);
                    }
                    self.child(inbound, key, update);
                }
            }
            _ => {}
        }
    }

    fn child_tool(&mut self, key: &str, block: &Value, inbound: &mut Inbound) {
        let (Some(id), Some(name)) = (
            block.get("id").and_then(Value::as_str),
            block.get("name").and_then(Value::as_str),
        ) else {
            return;
        };
        if self.child_tools.len() >= MAX_TRACKED_TOOLS || self.child_tools.contains_key(id) {
            return;
        }
        self.child_tools
            .insert(id.to_owned(), (key.to_owned(), name.to_owned()));
        let mut update = tool_fields(name, block.get("input").unwrap_or(&Value::Null));
        update.insert("sessionUpdate".into(), "tool_call".into());
        update.insert("toolCallId".into(), id.into());
        update.insert("status".into(), "pending".into());
        self.child(inbound, key, update.into());
    }

    fn child(&self, inbound: &mut Inbound, key: &str, update: Value) {
        inbound.children.push(ChildEvent::Update {
            key: key.to_owned(),
            update: self.update(update),
        });
    }

    /// The subagent's `Task` result closes its transcript; tools it never
    /// reported on end as failed.
    pub(super) fn end_child(&mut self, key: &str, outcome: ChildOutcome, inbound: &mut Inbound) {
        if !self.subagents.remove(key) {
            return;
        }
        let open: Vec<_> = self
            .child_tools
            .iter()
            .filter(|(_, (owner, _))| owner == key)
            .map(|(id, (_, name))| (id.clone(), name.clone()))
            .collect();
        for (id, name) in open {
            self.child_tools.remove(&id);
            self.child(
                inbound,
                key,
                json!({
                    "sessionUpdate": "tool_call_update", "toolCallId": id, "status": "failed",
                    "_meta": {"claudeCode": {"toolName": name}},
                }),
            );
        }
        inbound.children.push(ChildEvent::Ended {
            key: key.to_owned(),
            outcome,
        });
    }

    /// The turn ended: subagents still open stop with it.
    pub(super) fn end_children(&mut self, inbound: &mut Inbound) {
        let open: Vec<_> = self.subagents.iter().cloned().collect();
        for key in open {
            self.end_child(&key, ChildOutcome::Stopped, inbound);
        }
    }
}
