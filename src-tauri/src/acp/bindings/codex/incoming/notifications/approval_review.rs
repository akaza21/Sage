//! `item/autoApprovalReview/started` and `.../completed`: Codex's automatic
//! approval reviewer deciding a request in place of the user. Such a request
//! never reaches Sage as an approval, so there is no permission row to carry
//! the reviewer's state; its verdict becomes one session notice instead. The
//! start is identity-checked only, so each review leaves a single notice.
//! The payload is marked unstable upstream, so unknown action types and
//! statuses are tolerated.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::acp::intent::session_event::SessionNoticeLevel;

use super::super::{CodexAdapter, IngressDisposition, IngressError, support::text};
use super::notices::{human, notice, single, turn_session};

pub(super) fn approval_review(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
    completed: bool,
) -> Result<IngressDisposition, IngressError> {
    text(params, "reviewId", 512)?;
    let object = |key| {
        params
            .get(key)
            .and_then(Value::as_object)
            .ok_or(IngressError::Malformed)
    };
    let review = object("review")?;
    let status = text(review, "status", 32)?;
    let action = text(object("action")?, "type", 64)?;
    let Some(session) = turn_session(adapter, params)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    let (level, verdict) = match status {
        _ if !completed => return Ok(IngressDisposition::IgnoredLate),
        "approved" => (SessionNoticeLevel::Info, "approved"),
        "denied" => (SessionNoticeLevel::Warning, "denied"),
        "timedOut" => (SessionNoticeLevel::Warning, "timed out reviewing"),
        "aborted" => (SessionNoticeLevel::Info, "stopped reviewing"),
        _ => return Ok(IngressDisposition::IgnoredLate),
    };
    let subject = match action {
        "command" | "execve" => "a command",
        "writeStdin" => "input to a running command",
        "applyPatch" => "a file change",
        "networkAccess" => "network access",
        "mcpToolCall" => "an MCP tool call",
        "requestPermissions" => "a permission request",
        _ => "a request",
    };
    let mut message = format!("Codex's approval reviewer {verdict} {subject}");
    if let Some(risk @ ("low" | "medium" | "high" | "critical")) =
        review.get("riskLevel").and_then(Value::as_str)
    {
        let _ = write!(message, " (risk: {risk})");
    }
    message.push('.');
    // The rationale is optional; one that is absent, null or blank is left out.
    if let Ok(rationale) = human(review, "rationale") {
        let _ = write!(message, " {rationale}");
    }
    single(session, notice(level, "codex approval reviewer", message)?)
}
