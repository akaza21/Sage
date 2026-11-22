use super::{
    CodexAdapter, ID_BYTES, IngressDisposition, IngressError, REAUTHENTICATION_REQUIRED,
    RequiredText, SessionNoticeLevel, SessionScopedEvent, clean, human, identity, notice, single,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Codex keeps running when an MCP server fails to start, so report the
/// missing tools without ending the session.
pub(in super::super) fn mcp_startup_failed(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    server: &str,
    thread: &str,
) -> Result<IngressDisposition, IngressError> {
    let problem_identity = format!("mcp-start:{:x}", Sha256::digest(server.as_bytes()));
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
    let mut notice = notice(SessionNoticeLevel::Warning, "codex mcp", message)?;
    if let SessionScopedEvent::Notice(record) = &mut notice {
        record.identity = Some(
            RequiredText::new("noticeIdentity", problem_identity)
                .map_err(|_| IngressError::Content)?,
        );
    }
    if adapter.bound_thread_id().is_none() {
        adapter.buffer_session_notice(notice);
        return Ok(IngressDisposition::IgnoredLate);
    }
    single(adapter.thread_session_id(thread).map_err(identity)?, notice)
}
