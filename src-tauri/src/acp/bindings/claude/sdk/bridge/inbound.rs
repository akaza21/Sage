//! SDK frames from Claude Code, read as ACP session traffic.

use serde_json::{Value, json};

use super::{
    BridgeError, Inbound, MAX_TRACKED_TOOLS, Pending, RECENT_STREAMED_MESSAGES, SdkBridge,
    acp_error, acp_result,
};
use crate::acp::bindings::claude::runtime::ChildOutcome;
use crate::acp::bindings::claude::sdk::{
    config::{EFFORT_ID, MODE_ID, MODEL_ID, SessionSettings},
    tools::{plan_entries, result_text, text_content, tool_fields},
};

/// JSON-RPC codes the session reads: a failed request, and the sign-in
/// refusal Claude's ACP surface also uses.
const INTERNAL_ERROR: i64 = -32603;
const AUTH_REQUIRED: i64 = -32000;
/// Sage launches in `default`; any other starting mode would make the
/// mode picker misstate whether approvals are asked for.
const UNKNOWN_MODE: &str = "Claude Code started in a permission mode Sage cannot show";

impl SdkBridge {
    pub(in crate::acp::bindings::claude::sdk) fn inbound(
        &mut self,
        sdk: Value,
    ) -> Result<Inbound, BridgeError> {
        let mut inbound = Inbound::default();
        let nested = !sdk.get("parent_tool_use_id").is_none_or(Value::is_null);
        match sdk.get("type").and_then(Value::as_str) {
            Some("control_response") => self.control_response(&sdk["response"], &mut inbound)?,
            Some("control_request") => self.control_request(&sdk, &mut inbound)?,
            Some("control_cancel_request") => {
                let request_id = sdk.get("request_id").and_then(Value::as_str);
                self.withdraw(request_id.ok_or(BridgeError::Frame)?, &mut inbound);
            }
            // Results still finish the rows a subagent's approvals opened.
            Some("user") => {
                self.tool_results(&sdk["message"], &mut inbound);
                if nested {
                    self.nested(&sdk, &mut inbound);
                }
            }
            Some(_) if nested => self.nested(&sdk, &mut inbound),
            Some("stream_event") => self.stream_event(&sdk["event"], &mut inbound),
            Some("assistant") => self.assistant(&sdk, &mut inbound),
            Some("result") => self.result(&sdk, &mut inbound),
            Some("system") => self.system(&sdk, &mut inbound),
            Some("rate_limit_event") => Self::rate_limit(&sdk, &mut inbound),
            Some("command_lifecycle") => self.lifecycle(&sdk, &mut inbound),
            Some(_) => {}
            None => return Err(BridgeError::Frame),
        }
        Ok(inbound)
    }

    fn control_response(
        &mut self,
        response: &Value,
        inbound: &mut Inbound,
    ) -> Result<(), BridgeError> {
        let request_id = response
            .get("request_id")
            .and_then(Value::as_str)
            .ok_or(BridgeError::Frame)?;
        let pending = self.pending.remove(request_id).ok_or(BridgeError::Frame)?;
        if response.get("subtype").and_then(Value::as_str) != Some("success") {
            let message = response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Claude Code refused the request");
            match &pending {
                Pending::Control => Self::controlled(Err(message), inbound),
                Pending::Fork(_) => self.forked(None, inbound),
                Pending::Withdraw(uuid) => self.withdrawn(uuid, inbound),
                _ => {}
            }
            if let Some(id) = pending.acp_id() {
                inbound.acp.push(acp_error(id, INTERNAL_ERROR, message));
            }
            return Ok(());
        }
        let reply = response.get("response").unwrap_or(&Value::Null);
        match pending {
            Pending::Initialize(id) => {
                self.settings =
                    SessionSettings::from_initialize(reply, self.launch_model.as_deref());
                if self.settings.is_none() {
                    inbound
                        .acp
                        .push(acp_error(id, INTERNAL_ERROR, UNKNOWN_MODE));
                    return Ok(());
                }
                self.commands = super::commands::commands(reply);
                inbound.acp.push(acp_result(
                    id,
                    json!({
                        "protocolVersion": 1,
                        "agentCapabilities": {
                            "loadSession": false,
                            "promptCapabilities": {"image": true, "embeddedContext": true},
                            "sessionCapabilities": {"resume": {}},
                        },
                        "authMethods": [],
                    }),
                ));
            }
            Pending::Session(id) => {
                let settings = self.settings.as_ref().ok_or(BridgeError::Frame)?;
                inbound
                    .acp
                    .push(acp_result(id, settings.session_result(&self.session_id)));
                inbound.acp.push(self.update(json!({
                    "sessionUpdate": "available_commands_update",
                    "availableCommands": self.commands,
                })));
            }
            Pending::Mode { id, mode } => {
                self.settings
                    .as_mut()
                    .ok_or(BridgeError::Frame)?
                    .mode
                    .clone_from(&mode);
                inbound.acp.push(acp_result(id, json!({})));
                inbound.acp.push(self.update(
                    json!({"sessionUpdate": "current_mode_update", "currentModeId": mode}),
                ));
            }
            Pending::Option {
                id,
                config_id,
                value,
            } => {
                let settings = self.settings.as_mut().ok_or(BridgeError::Frame)?;
                match config_id.as_str() {
                    MODE_ID => settings.mode = value,
                    MODEL_ID => settings.select_model(&value),
                    EFFORT_ID => settings.effort = value,
                    _ => return Err(BridgeError::Frame),
                }
                inbound.acp.push(acp_result(
                    id,
                    json!({"configOptions": settings.config_options()}),
                ));
            }
            Pending::Interrupt => {}
            Pending::Withdraw(uuid) => self.withdrawn(&uuid, inbound),
            Pending::Control => Self::controlled(Ok(reply), inbound),
            Pending::Fork(config) => self.forked(config.as_deref(), inbound),
        }
        Ok(())
    }

    /// `can_use_tool` becomes an ACP permission request; Sage answers every
    /// other control request it does not serve with an error, so Claude Code
    /// never waits on it.
    fn control_request(&mut self, sdk: &Value, inbound: &mut Inbound) -> Result<(), BridgeError> {
        let request_id = sdk
            .get("request_id")
            .and_then(Value::as_str)
            .ok_or(BridgeError::Frame)?;
        let request = &sdk["request"];
        if request.get("subtype").and_then(Value::as_str) != Some("can_use_tool") {
            inbound.replies.push(json!({
                "type": "control_response",
                "response": {"subtype": "error", "request_id": request_id, "error": "Sage does not serve this request"},
            }));
            return Ok(());
        }
        self.can_use_tool(request_id, request, inbound)
    }

    fn stream_event(&mut self, event: &Value, inbound: &mut Inbound) {
        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                self.end_text();
                if let Some(id) = event.pointer("/message/id").and_then(Value::as_str) {
                    if self.streamed.len() >= RECENT_STREAMED_MESSAGES {
                        self.streamed.pop_front();
                    }
                    self.streamed.push_back(id.to_owned());
                }
            }
            Some("content_block_delta") => {
                let delta = &event["delta"];
                let (kind, text) = match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => ("agent_message_chunk", delta.get("text")),
                    Some("thinking_delta") => ("agent_thought_chunk", delta.get("thinking")),
                    _ => return,
                };
                if let Some(text) = text.and_then(Value::as_str).filter(|text| !text.is_empty()) {
                    let text = if kind == "agent_message_chunk" {
                        self.reply_text(text)
                    } else {
                        text.to_owned()
                    };
                    inbound.acp.push(self.update(
                        json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}}),
                    ));
                }
            }
            _ => {}
        }
    }

    /// A finished assistant message. Its text already streamed unless Claude
    /// Code produced it itself (errors such as a sign-in failure); its tool
    /// uses start tool rows, and `TodoWrite` replaces the plan.
    fn assistant(&mut self, sdk: &Value, inbound: &mut Inbound) {
        if sdk.get("error").and_then(Value::as_str) == Some("authentication_failed") {
            self.auth_failed = true;
        }
        let message = &sdk["message"];
        let streamed = message
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| self.streamed.iter().any(|streamed| streamed == id));
        if !streamed {
            self.end_text();
        }
        for block in message
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match block.get("type").and_then(Value::as_str) {
                Some("text") if !streamed => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        let text = self.reply_text(text);
                        inbound.acp.push(self.update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})));
                    }
                }
                Some("tool_use") => self.tool_use(block, inbound),
                _ => {}
            }
        }
    }

    fn tool_use(&mut self, block: &Value, inbound: &mut Inbound) {
        let (Some(id), Some(name)) = (
            block.get("id").and_then(Value::as_str),
            block.get("name").and_then(Value::as_str),
        ) else {
            return;
        };
        let input = block.get("input").unwrap_or(&Value::Null);
        if self.tools.contains_key(id) || self.subagents.contains(id) {
            return;
        }
        if self.spawn(id, name, input, inbound) {
            return;
        }
        if let Some(entries) = plan_entries(name, input) {
            inbound
                .acp
                .push(self.update(json!({"sessionUpdate": "plan", "entries": entries})));
            return;
        }
        // A row is shown only while it can be finished, and once: an
        // approval may have opened it already.
        if self.tools.len() >= MAX_TRACKED_TOOLS {
            return;
        }
        self.tools.insert(id.to_owned(), name.to_owned());
        let mut update = tool_fields(name, input);
        update.insert("sessionUpdate".into(), "tool_call".into());
        update.insert("toolCallId".into(), id.into());
        update.insert("status".into(), "pending".into());
        inbound.acp.push(self.update(update.into()));
    }

    /// `tool_result` blocks finish their tool rows. Edits keep the diff they
    /// started with; other tools show their output text.
    fn tool_results(&mut self, message: &Value, inbound: &mut Inbound) {
        for block in message
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let Some(id) = block.get("tool_use_id").and_then(Value::as_str) else {
                continue;
            };
            if self.subagents.contains(id) {
                let failed = block.get("is_error").and_then(Value::as_bool) == Some(true);
                let outcome = if failed {
                    ChildOutcome::Failed
                } else {
                    ChildOutcome::Completed
                };
                self.end_child(id, outcome, inbound);
                continue;
            }
            let Some(name) = self.tools.remove(id) else {
                continue;
            };
            let failed = block.get("is_error").and_then(Value::as_bool) == Some(true);
            let mut update = json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": id,
                "status": if failed { "failed" } else { "completed" },
                "_meta": {"claudeCode": {"toolName": name}},
            });
            let edits = tool_fields(&name, &Value::Null)["kind"] == "edit";
            if (failed || !edits)
                && let Some(text) = result_text(block.get("content"))
            {
                update["content"] = json!([text_content(&text)]);
            }
            inbound.acp.push(self.update(update));
        }
    }

    /// `result` completes the running prompt, with the context meter first.
    /// Open approvals are cancelled, and tool rows Claude Code never
    /// reported on end as failed.
    pub(super) fn result(&mut self, sdk: &Value, inbound: &mut Inbound) {
        if self.prompt.is_none() || self.holds_result(sdk) {
            return;
        }
        let Some(id) = self.prompt.take() else {
            return;
        };
        self.end_steers(inbound);
        for (tool, name) in std::mem::take(&mut self.tools) {
            inbound.acp.push(self.update(json!({
                "sessionUpdate": "tool_call_update", "toolCallId": tool, "status": "failed",
                "_meta": {"claudeCode": {"toolName": name}},
            })));
        }
        self.withdraw_open(inbound);
        self.end_retry(inbound);
        self.end_children(inbound);
        let usage = &sdk["usage"];
        let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        let (input, output) = (count("input_tokens"), count("output_tokens"));
        let (cache_read, cache_write) = (
            count("cache_read_input_tokens"),
            count("cache_creation_input_tokens"),
        );
        let total = input + output + cache_read + cache_write;
        if let Some(size) = context_window(sdk) {
            inbound.acp.push(
                self.update(json!({"sessionUpdate": "usage_update", "used": total, "size": size})),
            );
        }
        let failed = sdk.get("is_error").and_then(Value::as_bool) == Some(true);
        inbound.acp.push(if self.interrupted {
            acp_result(id, json!({"stopReason": "cancelled"}))
        } else if failed && self.auth_failed {
            acp_error(id, AUTH_REQUIRED, "Authentication required")
        } else if failed {
            acp_error(id, INTERNAL_ERROR, &failure_message(sdk))
        } else {
            acp_result(
                id,
                json!({
                    "stopReason": stop_reason(sdk),
                    "usage": {
                        "inputTokens": input, "outputTokens": output,
                        "cachedReadTokens": cache_read, "cachedWriteTokens": cache_write,
                        "totalTokens": total,
                    },
                }),
            )
        });
        self.interrupted = false;
        self.auth_failed = false;
        self.streamed.clear();
    }
}

impl Pending {
    fn acp_id(self) -> Option<Value> {
        match self {
            Self::Initialize(id)
            | Self::Session(id)
            | Self::Mode { id, .. }
            | Self::Option { id, .. } => Some(id),
            Self::Interrupt | Self::Withdraw(_) | Self::Control | Self::Fork(_) => None,
        }
    }
}

fn context_window(sdk: &Value) -> Option<u64> {
    sdk.get("modelUsage")?
        .as_object()?
        .values()
        .filter_map(|usage| usage.get("contextWindow").and_then(Value::as_u64))
        .max()
}

fn stop_reason(sdk: &Value) -> &'static str {
    match (
        sdk.get("subtype").and_then(Value::as_str),
        sdk.get("stop_reason").and_then(Value::as_str),
    ) {
        (Some("error_max_turns"), _) => "max_turn_requests",
        (_, Some("max_tokens")) => "max_tokens",
        (_, Some("refusal")) => "refusal",
        _ => "end_turn",
    }
}

fn failure_message(sdk: &Value) -> String {
    sdk.get("result")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            let errors = sdk.get("errors")?.as_array()?;
            let joined = errors
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            (!joined.is_empty()).then_some(joined)
        })
        .unwrap_or_else(|| "Claude Code stopped with an error.".to_owned())
}
