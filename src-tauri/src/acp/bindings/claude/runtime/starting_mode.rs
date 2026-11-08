//! The permission mode a new Claude session starts in.
//!
//! claude-agent-acp 0.101.0 starts a new session in `auto` when the model
//! supports it and no `permissions.defaultMode` is set, where 0.76.0 started
//! in `default`. Sage's access policy, not the adapter's default, decides the
//! starting mode: a new session whose mode catalog names another current mode
//! is switched with `session/set_mode` before it is reported ready.

use super::*;
use crate::acp::bindings::{
    access_policy::starting_native_session_mode, adapter::SessionPolicyKind,
    claude::manifest::CLAUDE_BINDING_ID,
};

impl ClaudeRuntime {
    /// The native mode of the binding's default access mode. Activation does
    /// not carry the session's requested access mode yet.
    fn starting_mode(&self) -> Option<&'static str> {
        let version = self
            .launch_binding
            .as_ref()
            .map_or("", |binding| binding.candidate().adapter_version.as_str());
        starting_native_session_mode(CLAUDE_BINDING_ID, version, self.binding_is_characterised())
    }

    /// The `session/set_mode` a new session needs before it is ready, or
    /// `None` when it already runs in Sage's mode. When Sage's access policy
    /// names a native mode, a session that reports no mode catalog, or a
    /// catalog without that mode, cannot honour the policy, so the session
    /// start fails closed and the half-started session is dropped.
    pub(super) fn starting_mode_request(
        &mut self,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let Some(mode) = self.starting_mode() else {
            return Ok(None);
        };
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        let honoured = match active.config.modes.as_ref() {
            None => Err(()),
            Some(modes) if modes.current_mode_id.as_str() == mode => return Ok(None),
            Some(_) => active.config.validate_mode(mode).map_err(|_| ()),
        };
        if honoured.is_err() {
            self.active = None;
            return Err(AdapterRuntimeError::ConfigMutationRejected);
        }
        let session = active.external_id.clone();
        self.reserve(
            PendingRequest::StartingMode(session.clone()),
            "session/set_mode",
            json!({"sessionId": session.as_str(), "modeId": mode}),
        )?
        .into_session_policy(SessionPolicyKind::StartingMode)
        .map(Some)
        .ok_or(AdapterRuntimeError::Frame)
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
