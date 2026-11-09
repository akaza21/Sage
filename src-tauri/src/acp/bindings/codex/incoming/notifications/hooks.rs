//! `hook/started` and `hook/completed`: each configured Codex hook handler is
//! one session activity row (kind `hook`) that runs and then completes or
//! fails. A hook may run outside any Turn (`turnId` is null), so the event is
//! session-scoped; the store records the row in the Turn whose batch carries
//! it, or in the previous Turn when Codex names that one.
//!
//! Codex names a run by its handler (event, order and source path), not by
//! its invocation, so a handler that runs again updates its own row and
//! concurrent runs of one handler share it. That name carries a local path,
//! so the row's identity is a digest of it and the path never leaves this
//! module; the label tells handlers apart by position and source instead.

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
const SOURCES: [(&str, &str); 11] = [
    ("system", "system"),
    ("user", "user"),
    ("project", "project"),
    ("mdm", "MDM"),
    ("sessionFlags", "session flags"),
    ("plugin", "plugin"),
    ("cloudRequirements", "cloud requirements"),
    ("cloudManagedConfig", "cloud managed config"),
    ("legacyManagedConfigFile", "legacy managed config file"),
    ("legacyManagedConfigMdm", "legacy managed config MDM"),
    ("unknown", "unknown"),
];

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
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    phase: HookPhase,
) -> Result<IngressDisposition, IngressError> {
    let thread = text(params, "threadId", 512)?;
    adapter.expect_thread(thread).map_err(identity)?;
    // The row is session-scoped, so a turn is validated but never required.
    let turn = match params.get("turnId") {
        None | Some(Value::Null) => None,
        Some(_) => Some(text(params, "turnId", 512)?),
    };
    let run = params
        .get("run")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let label = validate_run(run)?;
    let status = run_status(text(run, "status", 32)?)?;
    let state = match (phase, status) {
        (HookPhase::Started, RunStatus::Running) => SessionActivityState::Running,
        // A block is the hook doing its job; the line says what it blocked.
        (HookPhase::Completed, RunStatus::Completed | RunStatus::Blocked) => {
            SessionActivityState::Completed
        }
        (HookPhase::Completed, RunStatus::Failed) => SessionActivityState::Failed,
        (HookPhase::Completed, RunStatus::Stopped) => SessionActivityState::Cancelled,
        _ => return Err(IngressError::Malformed),
    };
    let progress = progress(run, status)?
        .map(|line| RequiredText::new("hookProgress", line))
        .transpose()
        .map_err(content)?;
    let row = SessionActivity {
        id: run_identity(text(run, "id", HOOK_PATH_BYTES)?)?,
        kind: RequiredText::new("activityKind", "hook").map_err(content)?,
        label: RequiredText::new("hookLabel", label).map_err(content)?,
        state,
        progress,
        // A run of the Turn that already ended belongs to that Turn's row,
        // not to the Turn now running.
        in_previous_turn: turn.is_some_and(|turn| adapter.turn_is_terminal(turn)),
        stoppable: false,
    };
    let in_active_turn = turn
        .is_some_and(|turn| adapter.expect_turn(turn).is_ok() && !adapter.turn_is_terminal(turn));
    match phase {
        HookPhase::Started if in_active_turn => adapter.track_running_hook(&row),
        HookPhase::Started => {}
        HookPhase::Completed => adapter.forget_running_hook(&row),
    }
    Ok(session_event(adapter, thread, row)?.into())
}

/// Settles the hooks still running when the Turn ends: their completion
/// will never arrive, so each row becomes cancelled.
pub(super) fn cancelled_hooks(
    adapter: &mut CodexAdapter,
    thread: &str,
) -> Result<Vec<IncomingEvent>, IngressError> {
    adapter
        .cancel_running_hooks()
        .into_iter()
        .map(|row| session_event(adapter, thread, row))
        .collect()
}

fn session_event(
    adapter: &CodexAdapter,
    thread: &str,
    row: SessionActivity,
) -> Result<IncomingEvent, IngressError> {
    Ok(IncomingEvent::Session {
        id: adapter.thread_session_id(thread).map_err(identity)?,
        event: SessionScopedEvent::Activity(row),
    })
}

/// Checks the run fields the row is built from and returns its label. The
/// row is display-only, so fields it never shows (handler type, execution
/// mode, scope, source path, command) are not required, and an event or
/// source a newer Codex adds is labelled generically instead of refused.
fn validate_run(run: &Map<String, Value>) -> Result<String, IngressError> {
    let event = text(run, "eventName", 64)?;
    let event_label = EVENT_NAMES
        .iter()
        .find(|(wire, _)| *wire == event)
        .map_or("Codex", |(_, label)| label);
    let source = match run.contains_key("source") {
        true => text(run, "source", 64)?,
        false => "unknown",
    };
    let source_label = SOURCES
        .iter()
        .find(|(wire, _)| *wire == source)
        .map_or("unknown", |(_, label)| label);
    let order = run
        .get("displayOrder")
        .and_then(Value::as_i64)
        .filter(|order| *order >= 0)
        .ok_or(IngressError::Malformed)?;
    integer(run.get("startedAt"))?;
    optional(run.get("completedAt"), integer)?;
    optional(run.get("durationMs"), integer)?;
    optional(run.get("statusMessage"), wire_text)?;
    for entry in entries(run)? {
        text(entry, "kind", 32)?;
        wire_text(entry.get("text"))?;
    }
    // Codex orders handlers from zero; the label counts from one.
    Ok(format!(
        "{event_label} hook #{} ({source_label})",
        order.saturating_add(1)
    ))
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
/// or stop so the row says why.
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
