use serde_json::{Value, json};

use super::usage::{context_window, turn_usage};
use super::{
    BridgeError, Inbound, MAX_TRACKED_TOOLS, Pending, RECENT_STREAMED_MESSAGES, SdkBridge,
    acp_auth_error, acp_error, acp_result,
};
use crate::acp::bindings::claude::runtime::ChildOutcome;
use crate::acp::bindings::claude::sdk::{
    config::{EFFORT_ID, MODE_ID, MODEL_ID, SessionSettings},
    tools::{plan_entries, result_text, text_content, tool_fields},
};

/// The JSON-RPC code for a failed request; `acp_auth_error` carries the
/// sign-in refusal.
const INTERNAL_ERROR: i64 = -32603;
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
        let kind = sdk.get("type").and_then(Value::as_str);
        if !nested && !self.observe_run(&sdk, &mut inbound) {
            return Ok(inbound);
        }
        match kind {
            Some("control_response") => self.control_response(&sdk["response"], &mut inbound)?,
            Some("control_request") => self.control_request(&sdk, &mut inbound)?,
            Some("control_cancel_request") => self.withdraw(&sdk, &mut inbound)?,
            // Results still finish the rows a subagent's approvals opened.
            Some("user") => {
                self.tool_results(&sdk, &mut inbound);
                if nested {
                    self.nested(&sdk, &mut inbound);
                }
            }
            Some(_) if nested => self.nested(&sdk, &mut inbound),
            Some("stream_event") => self.stream_event(&sdk["event"], &mut inbound),
            Some("assistant") => self.assistant(&sdk, &mut inbound),
            Some("result") => self.result(&sdk, &mut inbound),
            Some("system") => self.system(&sdk, &mut inbound),
            Some("active_goal") => self.active_goal(&sdk),
            Some("rate_limit_event") => Self::rate_limit(&sdk, &mut inbound),
            Some("command_lifecycle") => self.lifecycle(&sdk, &mut inbound),
            Some(_) => {}
            None => return Err(BridgeError::Frame),
        }
        self.continue_stop(&mut inbound);
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
        let pending = self.answered_request(request_id)?;
        if response.get("subtype").and_then(Value::as_str) != Some("success") {
            let message = response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Claude Code refused the request");
            match &pending {
                Pending::Commands => inbound.acp.push(self.update(
                    json!({"sessionUpdate":"available_commands_update","availableCommands":[]}),
                )),
                Pending::Control | Pending::Barrier | Pending::Restore => {
                    Self::controlled(Err(message), inbound);
                }
                Pending::Passive(request) => request.answered(Err(message), inbound),
                Pending::Fork(_) => self.forked(None, inbound),
                Pending::ForkAt(_, turn) => self.forked_at(None, turn.as_str(), inbound),
                Pending::Withdraw(uuid) => self.withdrawn(uuid, None, inbound),
                _ => {}
            }
            if let Some(id) = pending.acp_id() {
                inbound.acp.push(acp_error(id, INTERNAL_ERROR, message));
            }
            return Ok(());
        }
        let reply = response.get("response").unwrap_or(&Value::Null);
        match pending {
            Pending::Commands => {
                let commands = super::commands::commands(reply);
                inbound.acp.push(self.catalog_update(commands));
            }
            Pending::Initialize(id) => {
                self.settings =
                    SessionSettings::from_initialize(reply, self.launch_model.as_deref());
                if self.settings.is_none() {
                    inbound
                        .acp
                        .push(acp_error(id, INTERNAL_ERROR, UNKNOWN_MODE));
                    return Ok(());
                }
                self.commands = self.goal_support(super::commands::commands(reply));
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
                let commands = self.commands.take();
                inbound.acp.push(self.update(json!({
                    "sessionUpdate": "available_commands_update",
                    "availableCommands": commands,
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
            Pending::Interrupt => self.interrupted_idle(inbound),
            Pending::Withdraw(uuid) => self.withdrawn(&uuid, Some(reply), inbound),
            Pending::Control => Self::controlled(Ok(reply), inbound),
            Pending::Barrier => Self::controlled(Ok(&Value::Null), inbound),
            Pending::Restore => super::controls::file_plan(reply, inbound),
            Pending::Passive(request) => request.answered(Ok(reply), inbound),
            Pending::Abandoned => {}
            Pending::Fork(config) => self.forked(config.as_deref(), inbound),
            Pending::ForkAt(config, turn) => {
                self.forked_at(config.as_deref(), turn.as_str(), inbound)
            }
        }
        Ok(())
    }

    /// Sage answers every control request it does not serve with an error, so Claude Code never
    /// waits on it.
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
                    let mut update =
                        json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}});
                    // Each answer block is its own item, so text before and
                    // after a tool call stays apart instead of running on.
                    if kind == "agent_message_chunk" {
                        let block = event.get("index").and_then(Value::as_u64);
                        if let (Some(message), Some(block)) = (self.streamed.back(), block) {
                            update["itemId"] = json!(format!("{message}:{block}"));
                        }
                    }
                    inbound.acp.push(self.update(update));
                }
            }
            _ => {}
        }
    }

    /// Its text already streamed unless Claude Code produced it itself (errors such as a sign-in
    /// failure).
    fn assistant(&mut self, sdk: &Value, inbound: &mut Inbound) {
        if sdk.get("error").and_then(Value::as_str) == Some("authentication_failed") {
            self.auth_failed = true;
        }
        let message = &sdk["message"];
        self.track_context(&message["usage"], inbound);
        let streamed = message
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| self.streamed.iter().any(|streamed| streamed == id));
        let message_id = message.get("id").and_then(Value::as_str);
        for (index, block) in message
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            match block.get("type").and_then(Value::as_str) {
                Some("text") if !streamed => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        let mut update = json!({
                            "sessionUpdate": "agent_message_chunk",
                            "content": {"type": "text", "text": text},
                            "_meta": {"complete": true},
                        });
                        if let Some(message_id) = message_id {
                            update["itemId"] = json!(format!("{message_id}:{index}"));
                        }
                        inbound.acp.push(self.update(update));
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
        if self.tools.contains_key(id) || self.subagents.contains_key(id) {
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

    fn tool_results(&mut self, sdk: &Value, inbound: &mut Inbound) {
        for block in sdk["message"]
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
            let failed = block.get("is_error").and_then(Value::as_bool) == Some(true);
            let status = if failed { "failed" } else { "completed" };
            if self.subagents.contains_key(id) {
                if !self.keeps_open_after_launch(id, sdk.get("tool_use_result")) {
                    self.end_child(id, ChildOutcome::settled(Some(status)), inbound);
                }
                continue;
            }
            let Some(name) = self.tools.remove(id) else {
                continue;
            };
            let mut update = json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": id,
                "status": status,
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

    pub(super) fn result(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let Some(sdk) = self.turn_end(sdk) else {
            return;
        };
        let Some(id) = self.prompt.take() else {
            return;
        };
        self.end_steers(inbound);
        for (tool, name, status) in self.unfinished_tools() {
            inbound.acp.push(self.update(json!({
                "sessionUpdate": "tool_call_update", "toolCallId": tool, "status": status,
                "_meta": {"claudeCode": {"toolName": name}},
            })));
        }
        self.withdraw_open(inbound);
        self.end_retry(inbound);
        self.end_children(inbound);
        self.context_size = context_window(&sdk).or(self.context_size);
        if let Some(used) = self.context_tokens {
            self.report_context(used, inbound);
        }
        let failed = sdk.get("is_error").and_then(Value::as_bool) == Some(true);
        inbound.acp.push(if self.interrupted {
            acp_result(id, json!({"stopReason": "cancelled"}))
        } else if failed && self.auth_failed {
            acp_auth_error(id, failure_message(&sdk))
        } else if failed {
            acp_error(id, INTERNAL_ERROR, &failure_message(&sdk))
        } else {
            let mut result = json!({"stopReason": stop_reason(&sdk)});
            if let Some(usage) = turn_usage(&sdk["usage"]) {
                result["usage"] = usage;
            }
            acp_result(id, result)
        });
        self.interrupted = false;
        self.auth_failed = false;
        self.streamed.clear();
    }
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
