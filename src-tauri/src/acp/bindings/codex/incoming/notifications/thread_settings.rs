use serde_json::{Map, Value};

use crate::acp::contract::RequiredText;

use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError,
    support::{content, text},
};

/// `thread/settings/updated`: the settings Codex now runs a thread with.
/// Sage reads the model and effort of its own thread, which a `turn/start`
/// override or another client may have changed; another thread's settings
/// are not this session's.
pub(super) fn thread_settings(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let thread = text(params, "threadId", 512)?;
    if adapter.bound_thread_id() != Some(thread) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let settings = params
        .get("threadSettings")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let model =
        RequiredText::new("model", text(settings, "model", 512)?.to_owned()).map_err(content)?;
    let effort = match settings.get("effort") {
        None | Some(Value::Null) => None,
        Some(_) => Some(
            RequiredText::new("reasoningEffort", text(settings, "effort", 512)?.to_owned())
                .map_err(content)?,
        ),
    };
    Ok(IncomingEvent::ThreadSettings { model, effort }.into())
}
