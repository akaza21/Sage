use super::*;

impl ClaudeRuntime {
    /// `session_info_update` carries Claude's generated session title, and
    /// also metadata-only updates (`_meta`) that Sage does not project. A
    /// title becomes a session-scoped event, so it is kept whether or not a
    /// prompt is running.
    pub(super) fn session_info_notification(
        &self,
        value: &Value,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        let update = &value["params"]["update"];
        if update["sessionUpdate"] != "session_info_update" {
            return Ok(None);
        }
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        if value["params"]["sessionId"].as_str() != Some(active.external_id.as_str()) {
            return Err(AdapterRuntimeError::Frame);
        }
        let title = match update.get("title") {
            None | Some(Value::Null) => None,
            Some(Value::String(title)) => {
                crate::acp::bindings::session_title::harness_title_event(title)
            }
            Some(_) => return Err(AdapterRuntimeError::Frame),
        };
        let event = match title {
            Some(event) => SharedEvent::Session {
                id: self.session_identity(active.external_id.clone()),
                event,
            },
            None => SharedEvent::Absorbed,
        };
        AdapterBatch::new(vec![event], Vec::new()).map(Some)
    }
}
