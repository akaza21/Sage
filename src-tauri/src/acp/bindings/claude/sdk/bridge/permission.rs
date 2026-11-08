//! Tool approvals: Claude Code's `can_use_tool` requests as ACP permission
//! requests, and the user's decisions back as control responses.
//!
//! The session holds a card for every approval until the user's answer is
//! written. A request Claude Code withdraws (`control_cancel_request`) or a
//! turn outlives has its card cancelled. Requests outside a turn or beyond
//! the session's card limit are declined here, before the session sees them.

use serde_json::{Value, json};

use super::{BridgeError, Inbound, MAX_TRACKED_TOOLS, OPEN_APPROVALS, Permission, SdkBridge};
use crate::acp::bindings::claude::sdk::tools::tool_fields;

const ALLOW_ONCE: &str = "allow_once";
const ALLOW_ALWAYS: &str = "allow_always";
const REJECT: &str = "reject";
const DECLINED: &str = "The user declined this tool use.";
const TOO_MANY: &str = "Too many approvals are already waiting for the user.";
const NO_TURN: &str = "The user's turn has ended, so no one can approve this now.";
/// Claude's multiple-choice questions have no Sage card yet; declining with
/// this reason makes Claude ask in its reply instead.
const QUESTIONS_UNSUPPORTED: &str = "Sage cannot show multiple-choice questions yet. \
Ask the user the question in your reply instead.";

impl SdkBridge {
    pub(super) fn can_use_tool(
        &mut self,
        request_id: &str,
        request: &Value,
        inbound: &mut Inbound,
    ) -> Result<(), BridgeError> {
        let name = request
            .get("tool_name")
            .and_then(Value::as_str)
            .ok_or(BridgeError::Frame)?;
        if name == "AskUserQuestion" {
            inbound
                .replies
                .push(decision(request_id, deny(QUESTIONS_UNSUPPORTED)));
            return Ok(());
        }
        if self.prompt.is_none() {
            inbound.replies.push(decision(request_id, deny(NO_TURN)));
            return Ok(());
        }
        if self.permissions.len() + self.answering.len() >= OPEN_APPROVALS {
            inbound.replies.push(decision(request_id, deny(TOO_MANY)));
            return Ok(());
        }
        let input = request.get("input").cloned().unwrap_or_else(|| json!({}));
        let suggestions = request
            .get("permission_suggestions")
            .filter(|value| value.as_array().is_some_and(|rules| !rules.is_empty()))
            .cloned();
        let mut tool_call = tool_fields(name, &input);
        let tool_call_id = request
            .get("tool_use_id")
            .and_then(Value::as_str)
            .map_or_else(|| format!("permission-{request_id}"), str::to_owned);
        // A subagent's tool, or one past the tracked rows, has no row yet;
        // the card needs one, and the turn's end must be able to finish it.
        tool_call.insert("toolCallId".into(), tool_call_id.clone().into());
        tool_call.insert("status".into(), "pending".into());
        if !self.tools.contains_key(&tool_call_id) {
            if self.tools.len() >= MAX_TRACKED_TOOLS {
                inbound.replies.push(decision(request_id, deny(TOO_MANY)));
                return Ok(());
            }
            self.tools.insert(tool_call_id, name.to_owned());
            let mut row = tool_call.clone();
            row.insert("sessionUpdate".into(), "tool_call".into());
            inbound.acp.push(self.update(row.into()));
        }
        if let Some(title) = request.get("title").and_then(Value::as_str) {
            tool_call.insert("title".into(), title.into());
        }
        let mut options =
            vec![json!({"optionId": ALLOW_ONCE, "name": "Yes", "kind": "allow_once"})];
        if suggestions.is_some() {
            options.push(json!({"optionId": ALLOW_ALWAYS, "name": "Yes, and don't ask again", "kind": "allow_always"}));
        }
        options.push(json!({"optionId": REJECT, "name": "No", "kind": "reject_once"}));
        self.next_permission += 1;
        self.permissions.insert(
            self.next_permission,
            Permission {
                request_id: request_id.to_owned(),
                input,
                suggestions,
            },
        );
        inbound.acp.push(json!({
            "jsonrpc": "2.0",
            "id": self.next_permission,
            "method": "session/request_permission",
            "params": {"sessionId": self.session_id, "toolCall": tool_call, "options": options},
        }));
        Ok(())
    }

    /// Claude Code withdrew a request: its card is cancelled.
    pub(super) fn withdraw(&mut self, request_id: &str, inbound: &mut Inbound) {
        self.permissions.retain(|wire, permission| {
            let keep = permission.request_id != request_id;
            if !keep {
                inbound.withdrawn.push(*wire);
            }
            keep
        });
    }

    /// The turn ended: cards still open are cancelled.
    pub(super) fn withdraw_open(&mut self, inbound: &mut Inbound) {
        inbound
            .withdrawn
            .extend(std::mem::take(&mut self.permissions).into_keys());
    }

    /// A decision on a `can_use_tool` request. Allow keeps the tool's input;
    /// allow-always also adds Claude Code's suggested permission rules.
    pub(super) fn permission_reply(
        &mut self,
        id: &Value,
        result: Option<&Value>,
    ) -> Result<Value, BridgeError> {
        let wire = id.as_u64().ok_or(BridgeError::Unsupported)?;
        let permission = self
            .permissions
            .remove(&wire)
            .ok_or(BridgeError::Unsupported)?;
        let outcome = result
            .and_then(|result| result.get("outcome"))
            .ok_or(BridgeError::Unsupported)?;
        let selected = (outcome.get("outcome").and_then(Value::as_str) == Some("selected"))
            .then(|| outcome.get("optionId").and_then(Value::as_str))
            .flatten();
        let response = match selected {
            Some(ALLOW_ONCE) => json!({"behavior": "allow", "updatedInput": permission.input}),
            Some(ALLOW_ALWAYS) => json!({
                "behavior": "allow",
                "updatedInput": permission.input,
                "updatedPermissions": permission.suggestions.unwrap_or_else(|| json!([])),
            }),
            _ => deny(DECLINED),
        };
        self.answering.insert(permission.request_id.clone());
        Ok(decision(&permission.request_id, response))
    }
}

fn deny(message: &str) -> Value {
    json!({"behavior": "deny", "message": message})
}

fn decision(request_id: &str, response: Value) -> Value {
    json!({
        "type": "control_response",
        "response": {"subtype": "success", "request_id": request_id, "response": response},
    })
}
