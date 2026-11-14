//! Codex's automatic approval reviewer decides requests before they reach Sage
//! as approvals, so a denial or timeout needs a session notice.
//! The payload is marked unstable upstream, so a payload Sage cannot read is
//! ignored.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use sage_acp_protocol::intent::session_event::SessionNoticeLevel;

use super::super::{CodexAdapter, IngressDisposition, IngressError, support::text};
use super::notices::{human, informational_session, notice, single};

pub(super) fn approval_review(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    completed: bool,
) -> Result<IngressDisposition, IngressError> {
    if !completed {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let Some(session) = informational_session(adapter, params)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    let review = params.get("review").and_then(Value::as_object);
    let verdict = match field(review, "status") {
        Some("denied") => "denied",
        Some("timedOut") => "timed out reviewing",
        _ => return Ok(IngressDisposition::IgnoredLate),
    };
    let Ok(review_id) = text(params, "reviewId", 512) else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    if !adapter.complete_approval_review(text(params, "turnId", 512)?, review_id) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let subject = match field(params.get("action").and_then(Value::as_object), "type") {
        Some("command" | "execve") => "a command",
        Some("applyPatch") => "a file change",
        Some("networkAccess") => "network access",
        Some("mcpToolCall") => "an MCP tool call",
        _ => "a request",
    };
    let mut message = format!("Codex's approval reviewer {verdict} {subject}");
    if let Some(risk @ ("low" | "medium" | "high" | "critical")) = field(review, "riskLevel") {
        let _ = write!(message, " (risk: {risk})");
    }
    message.push('.');
    if let Some(rationale) = review.and_then(|review| human(review, "rationale").ok()) {
        let _ = write!(message, " {rationale}");
    }
    single(
        session,
        notice(
            SessionNoticeLevel::Warning,
            "codex approval reviewer",
            message,
        )?,
    )
}

fn field<'a>(object: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a str> {
    object?.get(key)?.as_str()
}
