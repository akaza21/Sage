//! `hook/started` and `hook/completed`: each configured Codex hook handler is
//! one session activity row (kind `hook`) that runs and then completes or
//! fails. A hook may run outside any Turn (`turnId` is null), so the row is
//! session-scoped rather than a transcript item.
//!
//! Codex names a run by its handler (event, order and source path), so a
//! handler that runs again updates its own row. That name carries a local
//! path, so the row's identity is a digest of it and the path never leaves
//! this module.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::acp::{
    bindings::codex::state::ExternalKind,
    contract::RequiredText,
    intent::{
        ExternalId,
        session_event::{
            SESSION_ACTIVITY_PROGRESS_BYTES, SessionActivity, SessionActivityState,
            SessionScopedEvent,
        },
    },
    limits::LIMITS,
};

use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError,
    support::{content, identity, text},
};
use super::status_line::status_line;

/// Upper bound of a hook run name or source path; both embed a local path.
const HOOK_PATH_BYTES: usize = 32 * 1024;

const EVENT_NAMES: [(&str, &str); 11] = [
    ("preToolUse", "PreToolUse"),
    ("permissionRequest", "PermissionRequest"),
    ("postToolUse", "PostToolUse"),
    ("preCompact", "PreCompact"),
    ("postCompact", "PostCompact"),
    ("sessionStart", "SessionStart"),
    ("sessionEnd", "SessionEnd"),
    ("userPromptSubmit", "UserPromptSubmit"),
    ("subagentStart", "SubagentStart"),
    ("subagentStop", "SubagentStop"),
    ("stop", "Stop"),
];
const HANDLER_TYPES: [&str; 3] = ["command", "prompt", "agent"];
const EXECUTION_MODES: [&str; 2] = ["sync", "async"];
const SCOPES: [&str; 2] = ["thread", "turn"];
const SOURCES: [&str; 11] = [
    "system",
    "user",
    "project",
    "mdm",
    "sessionFlags",
    "plugin",
    "cloudRequirements",
    "cloudManagedConfig",
    "legacyManagedConfigFile",
    "legacyManagedConfigMdm",
    "unknown",
];
const ENTRY_KINDS: [&str; 5] = ["warning", "stop", "feedback", "context", "error"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HookPhase {
    Started,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunStatus {
    Running,
    Completed,
    Failed,
    Blocked,
    Stopped,
}

pub(super) fn hook_run(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
    phase: HookPhase,
) -> Result<IngressDisposition, IngressError> {
    let thread = text(params, "threadId", 512)?;
    adapter.expect_thread(thread).map_err(identity)?;
    // The row is session-scoped, so a turn is validated but never required.
    match params.get("turnId") {
        None | Some(Value::Null) => {}
        Some(_) => {
            text(params, "turnId", 512)?;
        }
    }
    let run = params
        .get("run")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let label = validate_run(run)?;
    let status = run_status(text(run, "status", 32)?)?;
    let state = match (phase, status) {
        (HookPhase::Started, RunStatus::Running) => SessionActivityState::Running,
        (HookPhase::Completed, RunStatus::Completed) => SessionActivityState::Completed,
        (HookPhase::Completed, RunStatus::Failed | RunStatus::Blocked | RunStatus::Stopped) => {
            SessionActivityState::Failed
        }
        _ => return Err(IngressError::Malformed),
    };
    let progress = progress(run, status)?
        .map(|line| RequiredText::new("hookProgress", line))
        .transpose()
        .map_err(content)?;
    let event = SessionScopedEvent::Activity(SessionActivity {
        id: run_identity(text(run, "id", HOOK_PATH_BYTES)?)?,
        kind: RequiredText::new("activityKind", "hook").map_err(content)?,
        label: RequiredText::new("hookLabel", label).map_err(content)?,
        state,
        progress,
    });
    Ok(IncomingEvent::Session {
        id: adapter
            .scoped_id(ExternalKind::Session, thread)
            .map_err(identity)?,
        event,
    }
    .into())
}

/// Checks every field of the run summary and returns the row's label.
fn validate_run(run: &Map<String, Value>) -> Result<String, IngressError> {
    let event = text(run, "eventName", 64)?;
    let (_, event_label) = EVENT_NAMES
        .iter()
        .find(|(wire, _)| *wire == event)
        .ok_or(IngressError::Malformed)?;
    one_of(text(run, "handlerType", 32)?, &HANDLER_TYPES)?;
    one_of(text(run, "executionMode", 32)?, &EXECUTION_MODES)?;
    one_of(text(run, "scope", 32)?, &SCOPES)?;
    text(run, "sourcePath", HOOK_PATH_BYTES)?;
    if run.contains_key("source") {
        one_of(text(run, "source", 64)?, &SOURCES)?;
    }
    integer(run.get("displayOrder"))?;
    integer(run.get("startedAt"))?;
    optional(run.get("completedAt"), integer)?;
    optional(run.get("durationMs"), integer)?;
    optional(run.get("statusMessage"), wire_text)?;
    for entry in entries(run)? {
        one_of(text(entry, "kind", 32)?, &ENTRY_KINDS)?;
        wire_text(entry.get("text"))?;
    }
    Ok(format!("{event_label} hook"))
}

fn run_status(value: &str) -> Result<RunStatus, IngressError> {
    Ok(match value {
        "running" => RunStatus::Running,
        "completed" => RunStatus::Completed,
        "failed" => RunStatus::Failed,
        "blocked" => RunStatus::Blocked,
        "stopped" => RunStatus::Stopped,
        _ => return Err(IngressError::Malformed),
    })
}

/// The configured status line while a hook runs; afterwards its first
/// user-facing output (context entries are for the model), naming a block
/// or stop so a Failed row says why.
fn progress(run: &Map<String, Value>, status: RunStatus) -> Result<Option<String>, IngressError> {
    if status == RunStatus::Running {
        return Ok(run
            .get("statusMessage")
            .and_then(Value::as_str)
            .and_then(|message| status_line(message, SESSION_ACTIVITY_PROGRESS_BYTES)));
    }
    let prefix = match status {
        RunStatus::Blocked => "Blocked",
        RunStatus::Stopped => "Stopped",
        _ => "",
    };
    let budget = match prefix {
        "" => SESSION_ACTIVITY_PROGRESS_BYTES,
        prefix => SESSION_ACTIVITY_PROGRESS_BYTES - prefix.len() - ": ".len(),
    };
    let output = entries(run)?
        .find(|entry| entry.get("kind").and_then(Value::as_str) != Some("context"))
        .and_then(|entry| entry.get("text").and_then(Value::as_str))
        .and_then(|text| status_line(text, budget));
    Ok(match (prefix, output) {
        ("", output) => output,
        (prefix, Some(output)) => Some(format!("{prefix}: {output}")),
        (prefix, None) => Some(prefix.to_owned()),
    })
}

/// A stable, path-free identity for the hook handler's row.
fn run_identity(run_id: &str) -> Result<ExternalId, IngressError> {
    let digest = Sha256::digest(run_id.as_bytes());
    let hex = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    ExternalId::new("hookRunId", format!("hook-{hex}")).map_err(content)
}

fn entries(
    run: &Map<String, Value>,
) -> Result<impl Iterator<Item = &Map<String, Value>>, IngressError> {
    let entries = run
        .get("entries")
        .and_then(Value::as_array)
        .ok_or(IngressError::Malformed)?;
    if entries.iter().any(|entry| !entry.is_object()) {
        return Err(IngressError::Malformed);
    }
    Ok(entries.iter().filter_map(Value::as_object))
}

fn one_of(value: &str, allowed: &[&str]) -> Result<(), IngressError> {
    allowed
        .contains(&value)
        .then_some(())
        .ok_or(IngressError::Malformed)
}

fn integer(value: Option<&Value>) -> Result<(), IngressError> {
    value
        .and_then(Value::as_i64)
        .map(|_| ())
        .ok_or(IngressError::Malformed)
}

/// Hook output is the hook's own text; it is bounded by the wire frame and
/// clipped to one status line before it is shown.
fn wire_text(value: Option<&Value>) -> Result<(), IngressError> {
    value
        .and_then(Value::as_str)
        .filter(|text| text.len() <= LIMITS.wire.frame_bytes as usize && !text.contains('\0'))
        .map(|_| ())
        .ok_or(IngressError::Malformed)
}

fn optional(
    value: Option<&Value>,
    check: fn(Option<&Value>) -> Result<(), IngressError>,
) -> Result<(), IngressError> {
    match value {
        None | Some(Value::Null) => Ok(()),
        value => check(value),
    }
}
