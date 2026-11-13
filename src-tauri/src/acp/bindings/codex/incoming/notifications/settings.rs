//! `thread/settings/updated`: Codex changed the thread's settings. The
//! model and effort refresh the session's model configuration, as the
//! thread start reports them. Approvals and the sandbox follow Sage's own
//! access mode, which Sage sends with every turn, so Codex's copy of them
//! is not adopted.

use serde_json::{Map, Value};

use crate::acp::contract::RequiredText;

use super::super::{
    CodexAdapter, IngressDisposition, IngressError,
    support::{identity, text},
};

pub(super) fn thread_settings_updated(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    adapter
        .expect_thread(text(params, "threadId", 512)?)
        .map_err(identity)?;
    let settings = params
        .get("threadSettings")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let model = RequiredText::new("model", text(settings, "model", 512)?)
        .map_err(|_| IngressError::Malformed)?;
    let effort = match settings.get("effort") {
        None | Some(Value::Null) => None,
        Some(_) => Some(
            RequiredText::new("reasoningEffort", text(settings, "effort", 512)?)
                .map_err(|_| IngressError::Malformed)?,
        ),
    };
    Ok(IngressDisposition::SettingsUpdated { model, effort })
}
