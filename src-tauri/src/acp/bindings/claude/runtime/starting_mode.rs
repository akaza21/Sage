//! The permission mode a new Claude session starts in.
//!
//! claude-agent-acp 0.101.0 starts a new session in `auto` when the model
//! supports it and no `permissions.defaultMode` is set, where 0.76.0 started
//! in `default`. Sage's access policy, not the adapter's default, decides the
//! starting mode: a new session whose mode catalog names another current mode
//! is switched with `session/set_mode` before it is reported ready.

use super::*;
use crate::acp::bindings::{
    access_policy::starting_native_session_mode, claude::manifest::CLAUDE_BINDING_ID,
};

impl ClaudeRuntime {
    fn starting_mode(&self) -> Option<&'static str> {
        let version = self
            .launch_binding
            .as_ref()
            .map_or("", |binding| binding.candidate().adapter_version.as_str());
        starting_native_session_mode(CLAUDE_BINDING_ID, version, self.binding_is_characterised())
    }

    /// The `session/set_mode` a new session needs before it is ready, or
    /// `None` when it already runs in Sage's mode or reports no mode catalog.
    /// A catalog without Sage's mode cannot honour the access policy, so the
    /// session start fails closed.
    pub(super) fn starting_mode_request(
        &mut self,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let Some(mode) = self.starting_mode() else {
            return Ok(None);
        };
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        let Some(modes) = active.config.modes.as_ref() else {
            return Ok(None);
        };
        if modes.current_mode_id.as_str() == mode {
            return Ok(None);
        }
        active
            .config
            .validate_mode(mode)
            .map_err(|_| AdapterRuntimeError::ConfigMutationRejected)?;
        let session = active.external_id.clone();
        self.reserve(
            PendingRequest::StartingMode(session.clone()),
            "session/set_mode",
            json!({"sessionId": session.as_str(), "modeId": mode}),
        )
        .map(Some)
    }

    /// The new session is ready only once the adapter accepted Sage's mode.
    pub(super) fn starting_mode_response(
        &mut self,
        value: &Value,
        session: ExternalId,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let accepted =
            value.get("error").is_none() && value.get("result").is_some_and(Value::is_object);
        let mode = self.starting_mode().ok_or(AdapterRuntimeError::Frame)?;
        let active = self
            .active
            .as_mut()
            .filter(|active| active.external_id == session)
            .ok_or(AdapterRuntimeError::Frame)?;
        if !accepted {
            self.active = None;
            return Err(AdapterRuntimeError::ConfigMutationRejected);
        }
        let mode = crate::acp::session_config::ConfigId::new("modeId", mode)
            .map_err(|_| AdapterRuntimeError::Frame)?;
        active
            .config
            .set_current_mode(mode)
            .map_err(|_| AdapterRuntimeError::Frame)?;
        self.session_ready(session)
    }
}
