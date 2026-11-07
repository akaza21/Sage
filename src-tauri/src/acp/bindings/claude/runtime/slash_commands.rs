use super::*;
impl ClaudeRuntime {
    pub(super) fn commands_notification(
        &mut self,
        value: &Value,
    ) -> Result<bool, AdapterRuntimeError> {
        if value["params"]["update"]["sessionUpdate"] != "available_commands_update" {
            return Ok(false);
        }
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        crate::acp::bindings::standard_acp::surface::update_commands(
            value,
            active.external_id.as_str(),
            &mut active.commands,
        )
    }
}
