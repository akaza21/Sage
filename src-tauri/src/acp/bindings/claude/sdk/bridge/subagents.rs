//! Subagents: a `Task` tool use opens a child transcript, the frames Claude
//! Code tags with that tool use fill it, and the tool's result closes it. A
//! subagent running in the background outlives its tool result and the turn;
//! its `task_notification` closes it instead.

use serde_json::{Value, json};

use super::{Inbound, MAX_TRACKED_TOOLS, SdkBridge, notices::prose};
use crate::acp::bindings::claude::runtime::{ChildEvent, ChildOutcome, OPEN_CHILDREN};
use crate::acp::bindings::claude::sdk::tools::{result_text, text_content, tool_fields};

#[derive(Debug, Default)]
pub(super) struct Subagent {
    /// Claude Code's task id for the subagent, from its task events, its
    /// async launch result (`agentId`) or its own frames (`agent_id`).
    task_id: Option<String>,
    background: bool,
}

/// Claude Code's subagent tool, under its current and earlier name.
fn spawns(name: &str) -> bool {
    matches!(name, "Agent" | "Task")
}

impl SdkBridge {
    pub(super) fn spawn(
        &mut self,
        id: &str,
        name: &str,
        input: &Value,
        inbound: &mut Inbound,
    ) -> bool {
        // Past the runtime's open transcripts, a subagent stays a tool row.
        if !spawns(name) || self.subagents.len() >= OPEN_CHILDREN {
            return false;
        }
        let text = |key: &str, max| {
            input
                .get(key)
                .and_then(Value::as_str)
                .and_then(|text| prose(text, max))
        };
        self.subagents.insert(id.to_owned(), Subagent::default());
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

    /// Its streamed deltas are left out; the finished message carries the same text.
    pub(super) fn nested(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let Some(key) = sdk.get("parent_tool_use_id").and_then(Value::as_str) else {
            return;
        };
        let Some(subagent) = self.subagents.get_mut(key) else {
            return;
        };
        if let Some(agent) = sdk.get("agent_id").and_then(Value::as_str) {
            subagent.task_id.get_or_insert_with(|| agent.to_owned());
        }
        let message = &sdk["message"];
        let blocks = message
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        match sdk.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                // Claude Code sends each finished block as its own message
                // sharing the API message id; the frame's uuid tells them apart.
                let message_id = sdk
                    .get("uuid")
                    .or_else(|| message.get("id"))
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
                                    "_meta": {"complete": true},
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

    pub(super) fn end_child(&mut self, key: &str, outcome: ChildOutcome, inbound: &mut Inbound) {
        if self.subagents.remove(key).is_none() {
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

    /// How a finished turn closes its unfinished rows: failed, except a
    /// tool it allowed a background subagent to run. That row only carried
    /// the approval, so it completes with it; the tool's own row in the
    /// subagent's transcript takes its result, or fails when the subagent ends.
    pub(super) fn unfinished_tools(&mut self) -> Vec<(String, String, &'static str)> {
        let allowed = std::mem::take(&mut self.allowed_tools);
        std::mem::take(&mut self.tools)
            .into_iter()
            .map(|(tool, name)| {
                let delegated = allowed
                    .get(&tool)
                    .and_then(|agent| self.asking_subagent(agent.as_deref(), &tool))
                    .is_some_and(|subagent| subagent.background);
                let status = if delegated { "completed" } else { "failed" };
                (tool, name, status)
            })
            .collect()
    }

    /// The subagent that asked to run `tool`: the one Claude Code named by
    /// `agent`, its agent id, or else the one that reported the tool. Asked
    /// once the turn ends, when Sage has had the most frames to learn the id.
    fn asking_subagent(&self, agent: Option<&str>, tool: &str) -> Option<&Subagent> {
        agent
            .and_then(|agent| {
                self.subagents
                    .values()
                    .find(|subagent| subagent.task_id.as_deref() == Some(agent))
            })
            .or_else(|| {
                self.child_tools
                    .get(tool)
                    .and_then(|(owner, _)| self.subagents.get(owner))
            })
    }

    /// Ends the subagents a finished turn was waiting on; background ones run on.
    pub(super) fn end_children(&mut self, inbound: &mut Inbound) {
        let open: Vec<_> = self
            .subagents
            .iter()
            .filter(|(_, subagent)| !subagent.background)
            .map(|(key, _)| key.clone())
            .collect();
        for key in open {
            self.end_child(&key, ChildOutcome::Stopped, inbound);
        }
    }

    /// A subagent's tool result that only reports its launch into the
    /// background leaves its transcript open, and names the task whose
    /// `task_notification` will settle it.
    pub(super) fn keeps_open_after_launch(&mut self, key: &str, output: Option<&Value>) -> bool {
        let Some(subagent) = self.subagents.get_mut(key) else {
            return false;
        };
        let field = |name: &str| output.and_then(|output| output.get(name)?.as_str());
        if field("status") == Some("async_launched") {
            subagent.background = true;
            if let Some(agent) = field("agentId") {
                subagent.task_id.get_or_insert_with(|| agent.to_owned());
            }
        }
        subagent.background
    }

    /// Follows a subagent's background task: `task_started` names it,
    /// `task_updated` may move it to the background and `task_notification`
    /// ends it.
    pub(super) fn subagent_task(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let field = |key: &str| sdk.get(key).and_then(Value::as_str);
        let Some(task_id) = field("task_id") else {
            return;
        };
        let key = field("tool_use_id")
            .filter(|key| self.subagents.contains_key(*key))
            .map(str::to_owned)
            .or_else(|| {
                self.subagents
                    .iter()
                    .find(|(_, subagent)| subagent.task_id.as_deref() == Some(task_id))
                    .map(|(key, _)| key.clone())
            });
        let Some(key) = key else {
            return;
        };
        let backgrounded = sdk
            .get("is_backgrounded")
            .or_else(|| sdk["patch"].get("is_backgrounded"))
            .and_then(Value::as_bool)
            == Some(true);
        if let Some(subagent) = self.subagents.get_mut(&key) {
            subagent.task_id = Some(task_id.to_owned());
            subagent.background |= backgrounded;
        }
        if field("subtype") != Some("task_notification") {
            return;
        }
        self.end_child(&key, ChildOutcome::settled(field("status")), inbound);
    }
}
