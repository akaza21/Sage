//! Notices for child or fork threads are filtered earlier, in `incoming.rs`.
//! Only missing or wrongly typed required fields are malformed: fields or
//! enum values added upstream are tolerated, so a benign app-server addition
//! never tears the session down.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::acp::{
    bindings::codex::state::{ScopedExternalId, WarningKind},
    contract::RequiredText,
    intent::session_event::{
        SESSION_NOTICE_TEXT_BYTES, SessionNotice, SessionNoticeLevel, SessionScopedEvent,
    },
    redaction::text::{clip_display as clip, neutralized},
};

use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError,
    support::{identity, sanitize, text},
};

const ID_BYTES: usize = 512;
const MODEL_BYTES: usize = 512;
/// Bytes kept from each harness part (message, summary, details), so a long
/// summary cannot crowd the details out of the notice.
const PART_BYTES: usize = 448;
const FILE_NAME_BYTES: usize = 96;
const SAMPLE_FOLDERS: usize = 3;

pub(super) fn warning(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let thread = optional_text(params, "threadId", ID_BYTES)?;
    let message = human(params, "message")?;
    let notice = notice(
        SessionNoticeLevel::Warning,
        "codex",
        format!("Codex warning: {message}"),
    )?;
    // A named thread cannot be told apart from a foreign one before a
    // thread is bound, so it is ignored rather than failing.
    if thread.is_some() && adapter.bound_thread_id().is_none() {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let session = thread
        .map(|thread| adapter.thread_session_id(thread).map_err(identity))
        .transpose()?;
    if !adapter.note_warning(WarningKind::Generic, &message) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    match session {
        None => thread_less(adapter, notice),
        Some(session) => single(session, notice),
    }
}

pub(super) const REAUTHENTICATION_REQUIRED: &str = "reauthenticationRequired";

pub(super) fn mcp_startup_failed(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    server: &str,
    thread: &str,
) -> Result<IngressDisposition, IngressError> {
    let server = clean(server, ID_BYTES);
    let message = if params.get("failureReason").and_then(Value::as_str)
        == Some(REAUTHENTICATION_REQUIRED)
    {
        format!(
            "The MCP server {server} needs you to sign in again; its tools are unavailable until then."
        )
    } else {
        match human(params, "error") {
            Ok(error) => format!("The MCP server {server} failed to start: {error}"),
            Err(_) => format!("The MCP server {server} failed to start."),
        }
    };
    let notice = notice(SessionNoticeLevel::Warning, "codex mcp", message)?;
    if adapter.bound_thread_id().is_none() {
        adapter.buffer_session_notice(notice);
        return Ok(IngressDisposition::IgnoredLate);
    }
    single(adapter.thread_session_id(thread).map_err(identity)?, notice)
}

pub(super) fn guardian_warning(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let thread = text(params, "threadId", ID_BYTES)?;
    let notice = notice(
        SessionNoticeLevel::Warning,
        "codex approval reviewer",
        format!(
            "Codex approval reviewer warning: {}",
            human(params, "message")?
        ),
    )?;
    if adapter.bound_thread_id().is_none() {
        return Ok(IngressDisposition::IgnoredLate);
    }
    single(adapter.thread_session_id(thread).map_err(identity)?, notice)
}

pub(super) fn config_warning(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let summary = human(params, "summary")?;
    let mut message = format!("Codex configuration problem: {summary}");
    if let Some(name) = optional_text(params, "path", usize::MAX)?.and_then(file_name) {
        let _ = write!(message, "\nIn {name}");
        if let Some(line) = start_line(params) {
            let _ = write!(message, ", line {line}");
        }
    }
    let details = optional_human(params, "details")?;
    if let Some(details) = &details {
        let _ = write!(message, "\n{details}");
    }
    let notice = notice(SessionNoticeLevel::Warning, "codex config", message)?;
    let reported = match &details {
        Some(details) => format!("{summary}\n{details}"),
        None => summary,
    };
    if !adapter.note_warning(WarningKind::Config, &reported) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    thread_less(adapter, notice)
}

pub(super) fn deprecation_notice(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let mut message = format!("Codex deprecation notice: {}", human(params, "summary")?);
    if let Some(details) = optional_human(params, "details")? {
        let _ = write!(message, "\n{details}");
    }
    thread_less(
        adapter,
        notice(SessionNoticeLevel::Info, "codex deprecation", message)?,
    )
}

pub(super) fn model_rerouted(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let from = text(params, "fromModel", MODEL_BYTES)?;
    let to = text(params, "toModel", MODEL_BYTES)?;
    let reason = match text(params, "reason", usize::MAX)? {
        "highRiskCyberActivity" => "because it flagged possible high-risk cybersecurity activity",
        _ => "for a safety reason",
    };
    let Some(session) = turn_session(adapter, params)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    let model_id = RequiredText::new("modelId", to).map_err(|_| IngressError::Malformed)?;
    let notice = notice(
        SessionNoticeLevel::Warning,
        "codex model",
        format!(
            "Codex answered with {} instead of the requested {} {reason}.",
            clean(to, MODEL_BYTES),
            clean(from, MODEL_BYTES)
        ),
    )?;
    Ok(IngressDisposition::Events(vec![
        IncomingEvent::Session {
            id: session.clone(),
            event: SessionScopedEvent::TurnModel { model_id },
        },
        IncomingEvent::Session {
            id: session,
            event: notice,
        },
    ]))
}

pub(super) fn model_verification(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let trusted_access = params
        .get("verifications")
        .and_then(Value::as_array)
        .ok_or(IngressError::Malformed)?
        .iter()
        .any(|verification| verification.as_str() == Some("trustedAccessForCyber"));
    let Some(session) = turn_session(adapter, params)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    if !trusted_access {
        return Ok(IngressDisposition::IgnoredLate);
    }
    single(
        session,
        notice(
            SessionNoticeLevel::Warning,
            "codex model",
            "Codex flagged this chat for possible cybersecurity risk, so responses may be \
             slower while extra safety checks run. Verifying the account for Trusted Access \
             for Cyber authorizes security work."
                .to_owned(),
        )?,
    )
}

/// Deliberately not projected: a durable notice would outlive the transient
/// state it describes.
pub(super) fn model_safety_buffering(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    turn_session(adapter, params)?;
    Ok(IngressDisposition::IgnoredLate)
}

/// The completion carries no success flag, so its wording stays neutral.
pub(super) fn auth_recovery(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
    started: bool,
) -> Result<IngressDisposition, IngressError> {
    let Some(session) = informational_session(adapter, params)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    let Ok(provider) = text(params, "provider", MODEL_BYTES) else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    let lead = if started {
        "is renewing"
    } else {
        "finished renewing"
    };
    let mut message = format!(
        "Codex {lead} its sign-in to {}",
        clean(provider, MODEL_BYTES)
    );
    match human(params, "message") {
        Ok(detail) => _ = write!(message, ": {detail}"),
        Err(_) => message.push('.'),
    }
    single(
        session,
        notice(SessionNoticeLevel::Info, "codex sign-in", message)?,
    )
}

pub(super) fn world_writable_warning(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let samples = params
        .get("samplePaths")
        .and_then(Value::as_array)
        .ok_or(IngressError::Malformed)?;
    let extra = params
        .get("extraCount")
        .and_then(Value::as_u64)
        .ok_or(IngressError::Malformed)?;
    let failed_scan = params
        .get("failedScan")
        .and_then(Value::as_bool)
        .ok_or(IngressError::Malformed)?;
    let mut message = String::from(
        "Codex's Windows sandbox cannot protect folders that other users can write to.",
    );
    if !samples.is_empty() || extra > 0 {
        let total = extra.saturating_add(samples.len() as u64);
        let _ = write!(message, " It found {total}");
        let mut separator = ", including ";
        for path in samples.iter().take(SAMPLE_FOLDERS) {
            let path = path.as_str().ok_or(IngressError::Malformed)?;
            if let Some(name) = file_name(path) {
                let _ = write!(message, "{separator}{name}");
                separator = ", ";
            }
        }
        message.push('.');
    }
    if failed_scan {
        message.push_str("\nCodex could not finish checking for them.");
    }
    thread_less(
        adapter,
        notice(SessionNoticeLevel::Warning, "codex sandbox", message)?,
    )
}

/// A thread-less notice comes from the app-server process, which serves
/// exactly one bound thread, so it belongs to that thread's session. Before
/// a thread is bound it is held (bounded) and delivered when one binds.
fn thread_less(
    adapter: &mut CodexAdapter,
    notice: SessionScopedEvent,
) -> Result<IngressDisposition, IngressError> {
    match adapter.bound_thread_id() {
        Some(bound) => single(adapter.thread_session_id(bound).map_err(identity)?, notice),
        None => {
            adapter.buffer_session_notice(notice);
            Ok(IngressDisposition::IgnoredLate)
        }
    }
}

/// With no active Turn an unknown Turn is replayed history (as on resume) and
/// is ignored; while a Turn is active a different Turn is a foreign identity.
pub(super) fn turn_session(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<Option<ScopedExternalId>, IngressError> {
    let thread = text(params, "threadId", ID_BYTES)?;
    let turn = text(params, "turnId", ID_BYTES)?;
    if adapter.bound_thread_id().is_none() {
        return Ok(None);
    }
    let session = adapter.thread_session_id(thread).map_err(identity)?;
    if adapter.expect_turn(turn).is_ok() {
        Ok(Some(session))
    } else if adapter.has_active_turn() {
        Err(IngressError::Identity)
    } else {
        Ok(None)
    }
}

pub(super) fn informational_session(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<Option<ScopedExternalId>, IngressError> {
    match turn_session(adapter, params) {
        Err(IngressError::Malformed) => Ok(None),
        session => session,
    }
}

pub(super) fn single(
    session: ScopedExternalId,
    event: SessionScopedEvent,
) -> Result<IngressDisposition, IngressError> {
    Ok(IncomingEvent::Session { id: session, event }.into())
}

pub(super) fn notice(
    level: SessionNoticeLevel,
    source: &'static str,
    message: String,
) -> Result<SessionScopedEvent, IngressError> {
    let message = clip(&message, SESSION_NOTICE_TEXT_BYTES);
    Ok(SessionScopedEvent::Notice(SessionNotice {
        level,
        text: RequiredText::new("noticeText", message).map_err(|_| IngressError::Content)?,
        source: Some(RequiredText::new("noticeSource", source).map_err(|_| IngressError::Content)?),
    }))
}

fn optional_text<'a>(
    params: &'a Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<Option<&'a str>, IngressError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => text(params, key, max).map(Some),
    }
}

/// The raw value is already bounded by the frame limit.
pub(super) fn human(params: &Map<String, Value>, key: &str) -> Result<String, IngressError> {
    let value = params
        .get(key)
        .and_then(Value::as_str)
        .ok_or(IngressError::Malformed)?;
    // Neutralize first so hidden characters cannot split a secret past
    // redaction.
    let value = sanitize(&neutralized(value, true));
    let value = clip(value.trim(), PART_BYTES);
    if value.is_empty() {
        return Err(IngressError::Malformed);
    }
    Ok(value)
}

fn optional_human(params: &Map<String, Value>, key: &str) -> Result<Option<String>, IngressError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => human(params, key).map(Some),
    }
}

fn clean(value: &str, max: usize) -> String {
    clip(neutralized(value, true).trim(), max)
}

/// Only the file name of a config path is shown; the directory is local
/// detail the notice does not need.
fn file_name(path: &str) -> Option<String> {
    let name = clean(path.rsplit(['/', '\\']).next()?, FILE_NAME_BYTES);
    (!name.is_empty()).then_some(name)
}

/// The 1-based start line of a config `range`, when it has one.
fn start_line(params: &Map<String, Value>) -> Option<u64> {
    params
        .get("range")?
        .get("start")?
        .get("line")?
        .as_u64()
        .filter(|line| *line >= 1)
}
