//! Claude Code's `system` and `rate_limit_event` messages as session events:
//! notices (rate limits, model fallbacks, warnings), activities (compaction,
//! API retries, hooks, background tasks and subagents), mode changes, the
//! command list, and the output of local slash commands.
//!
//! Harness text is neutralized, redacted as prose and clipped. Messages and
//! fields Sage does not read are ignored, so a CLI addition never fails the
//! session.

use serde_json::{Value, json};

use super::{Inbound, SdkBridge};
use crate::acp::{
    contract::RequiredText,
    intent::{
        ModelId,
        session_event::{
            SESSION_ACTIVITY_LABEL_BYTES, SESSION_ACTIVITY_PROGRESS_BYTES,
            SESSION_NOTICE_TEXT_BYTES, SessionActivity, SessionActivityState, SessionNotice,
            SessionNoticeLevel, SessionScopedEvent,
        },
    },
    redaction::text::{Mode, neutralized, redact},
};

const SOURCE: &str = "claude code";
const RETRY_ACTIVITY: &str = "claude-api-retry";
const COMPACT_ACTIVITY: &str = "claude-compaction";
const ID_BYTES: usize = 256;
/// Activity ids of Claude Code's background tasks: the prefix, then the
/// task id.
pub(super) const TASK_PREFIX: &str = "task:";
const CLIPPED: &str = "\u{2026}";

impl SdkBridge {
    pub(super) fn system(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let field = |key: &str| sdk.get(key).and_then(Value::as_str);
        match field("subtype") {
            Some("status") => self.status(sdk, inbound),
            Some("compact_boundary") => {
                let meta = &sdk["compact_metadata"];
                let tokens = |key: &str| meta.get(key).and_then(Value::as_u64);
                let how = if meta["trigger"] == "manual" {
                    ""
                } else {
                    " automatically"
                };
                let text = match (tokens("pre_tokens"), tokens("post_tokens")) {
                    (Some(before), Some(after)) => format!(
                        "Claude Code compacted the conversation{how}, from {before} to {after} tokens."
                    ),
                    _ => format!("Claude Code compacted the conversation{how}."),
                };
                notice(inbound, SessionNoticeLevel::Info, &text);
            }
            Some("api_retry") => {
                let attempt = sdk.get("attempt").and_then(Value::as_u64).unwrap_or(1);
                let total = sdk
                    .get("max_retries")
                    .and_then(Value::as_u64)
                    .unwrap_or(attempt);
                let status = sdk
                    .get("error_status")
                    .and_then(Value::as_u64)
                    .map(|status| format!(" (HTTP {status})"))
                    .unwrap_or_default();
                self.retrying = true;
                activity(
                    inbound,
                    RETRY_ACTIVITY,
                    "retry",
                    &format!("Retrying the request to Claude{status}"),
                    SessionActivityState::Running,
                    Some(&format!("Attempt {attempt} of {total}")),
                );
            }
            Some("hook_started" | "hook_progress" | "hook_response") => hook(sdk, inbound),
            Some("task_started" | "task_progress" | "task_updated" | "task_notification") => {
                task(sdk, inbound);
            }
            Some("notification") => {
                let level = match field("priority") {
                    Some("high" | "immediate") => SessionNoticeLevel::Warning,
                    _ => SessionNoticeLevel::Info,
                };
                if let Some(text) = field("text") {
                    notice(inbound, level, text);
                }
            }
            Some("informational") => {
                let level = match field("level") {
                    Some("warning") => SessionNoticeLevel::Warning,
                    _ => SessionNoticeLevel::Info,
                };
                if let Some(text) = field("content") {
                    notice(inbound, level, text);
                }
            }
            Some("model_refusal_fallback") => {
                if let (Some(from), Some(to)) = (field("original_model"), field("fallback_model")) {
                    let text = format!("Claude Code switched from {from} to {to} after a refusal.");
                    notice(inbound, SessionNoticeLevel::Warning, &text);
                    if let Ok(model_id) = ModelId::new("modelId", clip(to, ID_BYTES)) {
                        inbound
                            .session
                            .push(SessionScopedEvent::TurnModel { model_id });
                    }
                }
            }
            Some("model_refusal_no_fallback") => {
                let text = field("content").unwrap_or("Claude declined to answer this request.");
                notice(inbound, SessionNoticeLevel::Error, text);
            }
            Some("permission_denied") => {
                if let Some(tool) = field("tool_name") {
                    let text = format!("Claude Code's automatic review denied {tool}.");
                    notice(inbound, SessionNoticeLevel::Warning, &text);
                }
            }
            Some("session_writes_lost") => notice(
                inbound,
                SessionNoticeLevel::Error,
                "Claude Code could not save this chat: the disk is full.",
            ),
            Some("local_command_output") => {
                if let Some(text) = field("content").filter(|text| !text.is_empty()) {
                    let update = json!({
                        "sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": text},
                    });
                    inbound.acp.push(self.update(update));
                }
            }
            Some("commands_changed") => {
                if let Some(commands) = sdk.get("commands") {
                    self.commands = super::inbound::commands(&json!({"commands": commands}));
                    let update = json!({
                        "sessionUpdate": "available_commands_update",
                        "availableCommands": self.commands,
                    });
                    inbound.acp.push(self.update(update));
                }
            }
            _ => {}
        }
    }

    /// Claude Code changes its own mode too, for example when an approval
    /// applies a `setMode` suggestion; follow it so the picker stays true.
    /// `compacting` runs as an activity until the status clears.
    fn status(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let compacting = sdk.get("status").and_then(Value::as_str) == Some("compacting");
        if compacting != self.compacting {
            self.compacting = compacting;
            let failed = sdk.get("compact_result").and_then(Value::as_str) == Some("failed");
            let state = match (compacting, failed) {
                (true, _) => SessionActivityState::Running,
                (false, true) => SessionActivityState::Failed,
                (false, false) => SessionActivityState::Completed,
            };
            let error = sdk.get("compact_error").and_then(Value::as_str);
            activity(
                inbound,
                COMPACT_ACTIVITY,
                "compaction",
                "Compacting the conversation",
                state,
                error,
            );
        }
        let mode = sdk.get("permissionMode").and_then(Value::as_str);
        let settings = self.settings.as_mut();
        let (Some(mode), Some(settings)) = (mode, settings) else {
            return;
        };
        if settings.has_mode(mode) && settings.mode != mode {
            settings.mode = mode.to_owned();
            let update = json!({"sessionUpdate": "current_mode_update", "currentModeId": mode});
            inbound.acp.push(self.update(update));
        }
    }

    /// A rate limit close to or past its cap; ordinary allowances are quiet.
    pub(super) fn rate_limit(sdk: &Value, inbound: &mut Inbound) {
        let info = &sdk["rate_limit_info"];
        let level = match info.get("status").and_then(Value::as_str) {
            Some("allowed_warning") => SessionNoticeLevel::Warning,
            Some("rejected") => SessionNoticeLevel::Error,
            _ => return,
        };
        let window = match info.get("rateLimitType").and_then(Value::as_str) {
            Some("five_hour") => "5-hour",
            Some("seven_day") => "weekly",
            Some("seven_day_opus") => "weekly Opus",
            Some("seven_day_sonnet") => "weekly Sonnet",
            Some("overage" | "seven_day_overage_included") => "extra usage",
            _ => "usage",
        };
        let used = info
            .get("utilization")
            .and_then(Value::as_f64)
            .map(|used| format!(" ({:.0}% used)", used * 100.0))
            .unwrap_or_default();
        let text = if level == SessionNoticeLevel::Error {
            format!("You've reached your Claude {window} limit{used}.")
        } else {
            format!("You're close to your Claude {window} limit{used}.")
        };
        notice(inbound, level, &text);
    }

    /// The turn ended: a retry still showing as running is over.
    pub(super) fn end_retry(&mut self, inbound: &mut Inbound) {
        if std::mem::take(&mut self.retrying) {
            let label = "Retrying the request to Claude";
            let state = SessionActivityState::Completed;
            activity(inbound, RETRY_ACTIVITY, "retry", label, state, None);
        }
    }
}

fn hook(sdk: &Value, inbound: &mut Inbound) {
    let field = |key: &str| sdk.get(key).and_then(Value::as_str);
    let (Some(id), Some(name)) = (field("hook_id"), field("hook_name")) else {
        return;
    };
    let state = match (field("subtype"), field("outcome")) {
        (Some("hook_response"), Some("success")) => SessionActivityState::Completed,
        (Some("hook_response"), Some("cancelled")) => SessionActivityState::Cancelled,
        (Some("hook_response"), _) => SessionActivityState::Failed,
        _ => SessionActivityState::Running,
    };
    let label = match field("hook_event") {
        Some(event) => format!("Hook {name} ({event})"),
        None => format!("Hook {name}"),
    };
    let output = field("stderr")
        .filter(|text| !text.trim().is_empty() && state == SessionActivityState::Failed)
        .or_else(|| field("output").filter(|text| !text.trim().is_empty()));
    activity(
        inbound,
        &format!("hook:{id}"),
        "hook",
        &label,
        state,
        output,
    );
}

/// Background tasks and subagents: one activity per task, carrying its
/// latest summary or tool.
fn task(sdk: &Value, inbound: &mut Inbound) {
    let field = |key: &str| sdk.get(key).and_then(Value::as_str);
    let Some(id) = field("task_id") else {
        return;
    };
    let status = field("status").or_else(|| sdk["patch"].get("status").and_then(Value::as_str));
    let state = match status {
        Some("completed") => SessionActivityState::Completed,
        Some("failed") => SessionActivityState::Failed,
        Some("stopped" | "killed") => SessionActivityState::Cancelled,
        _ => SessionActivityState::Running,
    };
    let kind = if field("subagent_type").is_some() {
        "subagent"
    } else {
        "task"
    };
    let description = field("description")
        .or_else(|| sdk["patch"].get("description").and_then(Value::as_str))
        .unwrap_or("Background task");
    let progress = field("summary")
        .map(str::to_owned)
        .or_else(|| field("last_tool_name").map(|tool| format!("Using {tool}")))
        .or_else(|| {
            sdk["patch"]
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    let id = format!("{TASK_PREFIX}{id}");
    activity(inbound, &id, kind, description, state, progress.as_deref());
}

fn notice(inbound: &mut Inbound, level: SessionNoticeLevel, text: &str) {
    let Some(text) = prose(text, SESSION_NOTICE_TEXT_BYTES) else {
        return;
    };
    let (Ok(text), Ok(source)) = (
        RequiredText::new("noticeText", text),
        RequiredText::new("noticeSource", SOURCE.to_owned()),
    ) else {
        return;
    };
    inbound
        .session
        .push(SessionScopedEvent::Notice(SessionNotice {
            level,
            text,
            source: Some(source),
        }));
}

fn activity(
    inbound: &mut Inbound,
    id: &str,
    kind: &'static str,
    label: &str,
    state: SessionActivityState,
    progress: Option<&str>,
) {
    let id_is_task = id.starts_with(TASK_PREFIX);
    let (Ok(id), Ok(kind), Some(Ok(label))) = (
        crate::acp::intent::ExternalId::new("activityId", clip(id, ID_BYTES)),
        RequiredText::new("activityKind", kind.to_owned()),
        prose(label, SESSION_ACTIVITY_LABEL_BYTES)
            .map(|label| RequiredText::new("activityLabel", label)),
    ) else {
        return;
    };
    let progress = progress
        .and_then(|text| prose(text, SESSION_ACTIVITY_PROGRESS_BYTES))
        .and_then(|text| RequiredText::new("activityProgress", text).ok());
    inbound
        .session
        .push(SessionScopedEvent::Activity(SessionActivity {
            id,
            kind,
            label,
            state,
            progress,
            // Claude Code stops one background task on request.
            stoppable: id_is_task,
            in_previous_turn: false,
        }));
}

/// Harness text as shown text: hidden characters dropped, secrets and local
/// paths redacted, clipped to `max` bytes. Blank text is not shown.
pub(super) fn prose(text: &str, max: usize) -> Option<String> {
    let text = neutralized(text, true);
    let text = clip(text.trim(), max);
    let text = redact("sessionNotice", &text, max.saturating_mul(2), Mode::Prose).ok()?;
    let text = clip(text.trim(), max);
    (!text.is_empty()).then_some(text)
}

fn clip(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max.saturating_sub(CLIPPED.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{CLIPPED}", &value[..end])
}
