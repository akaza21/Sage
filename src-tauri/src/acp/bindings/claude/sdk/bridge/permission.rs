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
const ANSWER: &str = "answer";
const ALLOW_ALWAYS: &str = "allow_always";
/// Allow-always on a file edit: Claude Code switches to accepting edits,
/// which Sage's mode guard admits for the rest of the turn.
const ALLOW_EDITS: &str = "allow-always-accept-edits";
const REJECT: &str = "reject";
/// Leaving plan mode, as the plan-exit options Sage's mode guard admits:
/// back to asking before edits, or into accepting them.
const EXIT_PLAN: [(&str, &str, &str, &str); 2] = [
    (
        "exit-plan-default",
        "allow_once",
        "Yes, and ask before edits",
        "default",
    ),
    (
        "exit-plan-accept-edits",
        "allow_always",
        "Yes, and accept edits",
        "acceptEdits",
    ),
];
const DECLINED: &str = "The user declined this tool use.";
const KEEP_PLANNING: &str = "The user chose to keep planning.";
const TOO_MANY: &str = "Too many approvals are already waiting for the user.";
const NO_TURN: &str = "The user's turn has ended, so no one can approve this now.";
/// Questions beyond what a Sage card shows; declining with this reason makes
/// Claude ask in its reply instead.
const QUESTIONS_UNSUPPORTED: &str = "Sage cannot show these questions. \
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
        if self.prompt.is_none() {
            inbound.replies.push(decision(request_id, deny(NO_TURN)));
            return Ok(());
        }
        if self.permissions.len() + self.answering.len() >= OPEN_APPROVALS {
            inbound.replies.push(decision(request_id, deny(TOO_MANY)));
            return Ok(());
        }
        let input = request.get("input").cloned().unwrap_or_else(|| json!({}));
        let questions = match name {
            "AskUserQuestion" => match super::questions::questions(&input) {
                Some((request, texts)) => Some((request, texts)),
                None => {
                    let reply = decision(request_id, deny(QUESTIONS_UNSUPPORTED));
                    inbound.replies.push(reply);
                    return Ok(());
                }
            },
            _ => None,
        };
        let suggestions = request
            .get("permission_suggestions")
            .and_then(Value::as_array)
            .map(|rules| admitted_suggestions(rules))
            .filter(|rules| !rules.is_empty());
        let accepts_edits = suggestions
            .as_ref()
            .is_some_and(|rules| rules.iter().any(|rule| rule["type"] == "setMode"));
        let suggestions = suggestions.map(Value::Array);
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
        let mut options = if name == "ExitPlanMode" {
            EXIT_PLAN
                .iter()
                .map(|(id, kind, label, _)| json!({"optionId": id, "name": label, "kind": kind}))
                .collect()
        } else {
            vec![json!({"optionId": ALLOW_ONCE, "name": "Yes", "kind": "allow_once"})]
        };
        let questions = questions.map(|(request, texts)| {
            inbound.questions = Some(request);
            options = vec![json!({"optionId": ANSWER, "name": "Answer", "kind": "allow_once"})];
            texts
        });
        if suggestions.is_some() && questions.is_none() && name != "ExitPlanMode" {
            options.push(if accepts_edits {
                json!({"optionId": ALLOW_EDITS, "name": "Yes, and accept edits for this turn", "kind": "allow_always"})
            } else {
                json!({"optionId": ALLOW_ALWAYS, "name": "Yes, and don't ask again", "kind": "allow_always"})
            });
        }
        options.push(json!({"optionId": REJECT, "name": "No", "kind": "reject_once"}));
        self.next_permission += 1;
        self.permissions.insert(
            self.next_permission,
            Permission {
                request_id: request_id.to_owned(),
                input,
                suggestions,
                questions,
                plan_exit: name == "ExitPlanMode",
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
        if let Some(texts) = &permission.questions {
            let cancelled = outcome.get("outcome").and_then(Value::as_str) == Some("cancelled");
            let answers = (selected == Some(ANSWER)).then(|| result.and_then(|r| r.get("answers")));
            let response = if cancelled {
                deny(DECLINED)
            } else {
                let input = super::questions::answered(&permission.input, texts, answers.flatten());
                json!({"behavior": "allow", "updatedInput": input})
            };
            self.answering.insert(permission.request_id.clone());
            return Ok(decision(&permission.request_id, response));
        }
        let response = match selected {
            Some(ALLOW_ONCE) => json!({"behavior": "allow", "updatedInput": permission.input}),
            Some(ALLOW_ALWAYS | ALLOW_EDITS) => json!({
                "behavior": "allow",
                "updatedInput": permission.input,
                "updatedPermissions": permission.suggestions.unwrap_or_else(|| json!([])),
            }),
            // Approving the plan leaves plan mode for the mode the user chose.
            Some(option) => match EXIT_PLAN.iter().find(|(id, ..)| *id == option) {
                Some((.., mode)) => json!({
                    "behavior": "allow",
                    "updatedInput": permission.input,
                    "updatedPermissions": [{"type": "setMode", "mode": mode, "destination": "session"}],
                }),
                None => deny(DECLINED),
            },
            // Declining a plan stops the turn so the user can say what to
            // change, rather than Claude guessing and asking again.
            None if permission.plan_exit => json!({
                "behavior": "deny", "message": KEEP_PLANNING, "interrupt": true,
            }),
            None => deny(DECLINED),
        };
        self.answering.insert(permission.request_id.clone());
        Ok(decision(&permission.request_id, response))
    }
}

/// Claude Code's suggested rules without any mode switch other than into
/// accepting edits, the only one a permission answer may widen a turn to.
fn admitted_suggestions(rules: &[Value]) -> Vec<Value> {
    rules
        .iter()
        .filter(|rule| rule["type"] != "setMode" || rule["mode"] == "acceptEdits")
        .cloned()
        .collect()
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
