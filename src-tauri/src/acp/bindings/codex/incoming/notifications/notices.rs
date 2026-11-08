//! Codex notices: harness warnings, configuration and deprecation notices,
//! and model reroutes, projected as session-scoped events.
//!
//! Each notice is a short Sage-authored lead plus the harness text, clipped
//! to `SESSION_NOTICE_TEXT_BYTES`. Nothing is retained here: the session
//! surface keeps the newest `SESSION_NOTICES_MAX` notices and already
//! collapses identical ones. The app-server version is pinned, so a field
//! outside the 0.146.0 schema is malformed.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::acp::{
    bindings::codex::state::ScopedExternalId,
    contract::RequiredText,
    intent::session_event::{
        SESSION_NOTICE_TEXT_BYTES, SessionNotice, SessionNoticeLevel, SessionScopedEvent,
    },
};

use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError,
    support::{identity, text},
};

const ID_BYTES: usize = 512;
const MODEL_BYTES: usize = 512;
const PATH_BYTES: usize = 4096;
/// Entries accepted in each `model/*` string list.
const LIST_MAX: usize = 16;
const LIST_ENTRY_BYTES: usize = 128;
const CLIPPED: &str = "\u{2026}";

/// `warning`: a harness warning, optionally aimed at one thread.
pub(super) fn warning(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    only(params, &["threadId", "message"])?;
    let thread = optional_text(params, "threadId", ID_BYTES)?;
    let message = human(params, "message")?;
    let Some(session) = session(adapter, thread)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    single_notice(
        session,
        SessionNoticeLevel::Warning,
        "codex",
        format!("Codex warning: {message}"),
    )
}

/// `guardianWarning`: a warning from Codex's automatic approval reviewer.
pub(super) fn guardian_warning(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    only(params, &["threadId", "message"])?;
    let thread = text(params, "threadId", ID_BYTES)?;
    let message = human(params, "message")?;
    let Some(session) = session(adapter, Some(thread))? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    single_notice(
        session,
        SessionNoticeLevel::Warning,
        "codex approval reviewer",
        format!("Codex approval reviewer warning: {message}"),
    )
}

/// `configWarning`: Codex could not apply part of its configuration.
pub(super) fn config_warning(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    only(params, &["summary", "details", "path", "range"])?;
    let mut message = format!("Codex configuration problem: {}", human(params, "summary")?);
    let line = start_line(params.get("range"))?;
    if let Some(path) = optional_text(params, "path", PATH_BYTES)? {
        let _ = write!(message, "\nIn {path}");
        if let Some(line) = line {
            let _ = write!(message, ", line {line}");
        }
    }
    if let Some(details) = optional_human(params, "details")? {
        let _ = write!(message, "\n{details}");
    }
    let Some(session) = session(adapter, None)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    single_notice(
        session,
        SessionNoticeLevel::Warning,
        "codex config",
        message,
    )
}

/// `deprecationNotice`: a Codex feature or setting in use is deprecated.
pub(super) fn deprecation_notice(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    only(params, &["summary", "details"])?;
    let mut message = format!("Codex deprecation notice: {}", human(params, "summary")?);
    if let Some(details) = optional_human(params, "details")? {
        let _ = write!(message, "\n{details}");
    }
    let Some(session) = session(adapter, None)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    single_notice(
        session,
        SessionNoticeLevel::Info,
        "codex deprecation",
        message,
    )
}

/// `model/rerouted`: Codex answered the turn with a different model than
/// the one Sage requested. The effective model becomes the session's model
/// and a notice names both, so the picker is never silently wrong.
pub(super) fn model_rerouted(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    only(
        params,
        &["threadId", "turnId", "fromModel", "toModel", "reason"],
    )?;
    let from = text(params, "fromModel", MODEL_BYTES)?;
    let to = text(params, "toModel", MODEL_BYTES)?;
    let reason = match text(params, "reason", 64)? {
        "highRiskCyberActivity" => "it flagged possible high-risk cybersecurity activity",
        _ => return Err(IngressError::Malformed),
    };
    let Some(session) = turn_session(adapter, params)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    let model_id = RequiredText::new("modelId", to).map_err(|_| IngressError::Malformed)?;
    let notice = notice(
        SessionNoticeLevel::Warning,
        "codex model",
        format!("Codex answered with {to} instead of the requested {from} because {reason}."),
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

/// `model/verification`: the backend recommends account verification
/// because the chat keeps being flagged. It explains slower responses and
/// what the user can do, so it is user-meaningful and becomes a notice.
pub(super) fn model_verification(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    only(params, &["threadId", "turnId", "verifications"])?;
    let verifications = list(params, "verifications")?;
    if verifications
        .iter()
        .any(|verification| *verification != "trustedAccessForCyber")
    {
        return Err(IngressError::Malformed);
    }
    let Some(session) = turn_session(adapter, params)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    if verifications.is_empty() {
        return Ok(IngressDisposition::IgnoredLate);
    }
    single_notice(
        session,
        SessionNoticeLevel::Warning,
        "codex model",
        "Codex flagged this chat for possible cybersecurity risk, so responses may be slower \
         while extra safety checks run. Verifying the account for Trusted Access for Cyber \
         authorizes security work."
            .to_owned(),
    )
}

/// `model/safetyBuffering/updated`: validated and deliberately not
/// projected. It toggles a transient "still reviewing" state within one
/// running turn (the turn already shows as running) and offers a retry on a
/// faster model that only Codex's own UI can perform. A durable notice
/// would outlive the state it describes.
pub(super) fn model_safety_buffering(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    only(
        params,
        &[
            "threadId",
            "turnId",
            "model",
            "useCases",
            "reasons",
            "showBufferingUi",
            "fasterModel",
        ],
    )?;
    text(params, "model", MODEL_BYTES)?;
    list(params, "useCases")?;
    list(params, "reasons")?;
    optional_text(params, "fasterModel", MODEL_BYTES)?;
    if !params.get("showBufferingUi").is_some_and(Value::is_boolean) {
        return Err(IngressError::Malformed);
    }
    turn_session(adapter, params)?;
    Ok(IngressDisposition::IgnoredLate)
}

/// The session a notice belongs to. A thread-less notice comes from the
/// app-server process, which serves exactly one bound thread, so it belongs
/// to that thread's session. Before any thread is bound no session can own
/// the notice, and a named thread cannot yet be told apart from a foreign
/// one, so the notice is ignored rather than failing the runtime. Once a
/// thread is bound, a notice for any other thread is a foreign identity.
fn session(
    adapter: &CodexAdapter,
    thread: Option<&str>,
) -> Result<Option<ScopedExternalId>, IngressError> {
    let Some(bound) = adapter.bound_thread_id() else {
        return Ok(None);
    };
    adapter
        .thread_session_id(thread.unwrap_or(bound))
        .map(Some)
        .map_err(identity)
}

/// The session of a turn-scoped `model/*` notification. It must name the
/// active or the just-finished Turn. With no active Turn an unknown Turn is
/// replayed history (as on resume) and is ignored; while a Turn is active a
/// different Turn is a foreign identity.
fn turn_session(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<Option<ScopedExternalId>, IngressError> {
    let thread = text(params, "threadId", ID_BYTES)?;
    let turn = text(params, "turnId", ID_BYTES)?;
    let Some(session) = session(adapter, Some(thread))? else {
        return Ok(None);
    };
    if adapter.expect_turn(turn).is_ok() {
        Ok(Some(session))
    } else if adapter.has_active_turn() {
        Err(IngressError::Identity)
    } else {
        Ok(None)
    }
}

fn single_notice(
    session: ScopedExternalId,
    level: SessionNoticeLevel,
    source: &'static str,
    message: String,
) -> Result<IngressDisposition, IngressError> {
    Ok(IncomingEvent::Session {
        id: session,
        event: notice(level, source, message)?,
    }
    .into())
}

fn notice(
    level: SessionNoticeLevel,
    source: &'static str,
    mut message: String,
) -> Result<SessionScopedEvent, IngressError> {
    if message.len() > SESSION_NOTICE_TEXT_BYTES {
        let mut end = SESSION_NOTICE_TEXT_BYTES - CLIPPED.len();
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push_str(CLIPPED);
    }
    Ok(SessionScopedEvent::Notice(SessionNotice {
        level,
        text: RequiredText::new("noticeText", message).map_err(|_| IngressError::Content)?,
        source: Some(RequiredText::new("noticeSource", source).map_err(|_| IngressError::Content)?),
    }))
}

/// Rejects any field outside `allowed`.
fn only(params: &Map<String, Value>, allowed: &[&str]) -> Result<(), IngressError> {
    if params.keys().all(|key| allowed.contains(&key.as_str())) {
        Ok(())
    } else {
        Err(IngressError::Malformed)
    }
}

/// An absent or null field is `None`; otherwise it must be valid text.
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

/// Human-readable harness text. Its length is bounded by the frame limit
/// and the notice clips it, so only emptiness and NUL are rejected here.
fn human<'a>(params: &'a Map<String, Value>, key: &str) -> Result<&'a str, IngressError> {
    let value = params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .ok_or(IngressError::Malformed)?;
    if value.is_empty() || value.contains('\0') {
        return Err(IngressError::Malformed);
    }
    Ok(value)
}

fn optional_human<'a>(
    params: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, IngressError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => human(params, key).map(Some),
    }
}

/// A bounded list of short identifiers.
fn list<'a>(params: &'a Map<String, Value>, key: &str) -> Result<Vec<&'a str>, IngressError> {
    let values = params
        .get(key)
        .and_then(Value::as_array)
        .filter(|values| values.len() <= LIST_MAX)
        .ok_or(IngressError::Malformed)?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| {
                    !value.is_empty() && value.len() <= LIST_ENTRY_BYTES && !value.contains('\0')
                })
                .ok_or(IngressError::Malformed)
        })
        .collect()
}

/// The 1-based start line of a config `range`, which must be exactly
/// `{start:{line,column}, end:{line,column}}` when present.
fn start_line(range: Option<&Value>) -> Result<Option<u64>, IngressError> {
    let Some(range) = range.filter(|range| !range.is_null()) else {
        return Ok(None);
    };
    let range = range.as_object().ok_or(IngressError::Malformed)?;
    only(range, &["start", "end"])?;
    let start = line(range, "start")?;
    line(range, "end")?;
    Ok(Some(start))
}

/// The line of one `{line, column}` position; both are 1-based.
fn line(range: &Map<String, Value>, key: &str) -> Result<u64, IngressError> {
    let position = range
        .get(key)
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    only(position, &["line", "column"])?;
    let [line, column] = ["line", "column"].map(|field| {
        position
            .get(field)
            .and_then(Value::as_u64)
            .filter(|value| *value >= 1)
    });
    column.and(line).ok_or(IngressError::Malformed)
}
