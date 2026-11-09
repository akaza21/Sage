//! SDK frames from Claude Code, read as ACP session traffic.

use serde_json::{Value, json};

use super::{
    BridgeError, Inbound, MAX_TRACKED_TOOLS, Pending, RECENT_STREAMED_MESSAGES, SdkBridge,
    acp_error, acp_result,
};
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
            Some("user") => self.tool_results(&sdk["message"], &mut inbound),
            // Other subagent traffic is projected in a later stage; the
            // parent's `Task` tool row carries the subagent's outcome.
            Some(_) if nested => {}
            Some("stream_event") => self.stream_event(&sdk["event"], &mut inbound),
            Some("assistant") => self.assistant(&sdk, &mut inbound),
            Some("result") => self.result(&sdk, &mut inbound),
            Some("system") => self.status(&sdk, &mut inbound),
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
                self.commands = commands(reply);
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

    /// Claude Code changes its own mode too, for example when an approval
    /// applies a `setMode` suggestion; follow it so the picker stays true.
    fn status(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let mode = sdk.get("permissionMode").and_then(Value::as_str);
        let settings = self.settings.as_mut();
        let (Some(mode), Some(settings)) = (mode, settings) else {
            return;
        };
        if sdk["subtype"] == "status" && settings.has_mode(mode) && settings.mode != mode {
            settings.mode = mode.to_owned();
            let update = json!({"sessionUpdate": "current_mode_update", "currentModeId": mode});
            inbound.acp.push(self.update(update));
        }
    }

    fn stream_event(&mut self, event: &Value, inbound: &mut Inbound) {
        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => {
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
        for block in message
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match block.get("type").and_then(Value::as_str) {
                Some("text") if !streamed => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
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
        if let Some(entries) = plan_entries(name, input) {
            inbound
                .acp
                .push(self.update(json!({"sessionUpdate": "plan", "entries": entries})));
            return;
        }
        // A row is shown only while it can be finished, and once: an
        // approval may have opened it already.
        if self.tools.len() >= MAX_TRACKED_TOOLS || self.tools.contains_key(id) {
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
    fn result(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let Some(id) = self.prompt.take() else {
            return;
        };
        for (tool, name) in std::mem::take(&mut self.tools) {
            inbound.acp.push(self.update(json!({
                "sessionUpdate": "tool_call_update", "toolCallId": tool, "status": "failed",
                "_meta": {"claudeCode": {"toolName": name}},
            })));
        }
        self.withdraw_open(inbound);
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
            Self::Interrupt => None,
        }
    }
}

/// Claude Code's commands as ACP `availableCommands`.
fn commands(reply: &Value) -> Value {
    reply
        .get("commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|command| {
            let hint = command.get("argumentHint").and_then(Value::as_str).filter(|hint| !hint.is_empty());
            Some(json!({
                "name": command.get("name")?.as_str()?,
                "description": command.get("description").and_then(Value::as_str).unwrap_or_default(),
                "input": hint.map(|hint| json!({"hint": hint})),
            }))
        })
        .collect()
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
