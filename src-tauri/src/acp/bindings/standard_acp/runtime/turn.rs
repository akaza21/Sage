use serde_json::json;

use crate::acp::bindings::adapter::{AdapterEgress, AdapterRuntimeError};
use sage_acp_protocol::contract::RequiredText;

use super::{
    DeferredPrompt, PendingRequest, StandardAcpRuntime, TurnIntent, map_translation, outbound,
};

impl StandardAcpRuntime {
    pub(super) fn prompt(
        &mut self,
        prompt: crate::acp::transcript_quote::Prompt,
        client_message_id: RequiredText<512>,
        access_mode: sage_acp_protocol::access_mode::SessionAccessMode,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if let Some(revision) = prompt.command_revision {
            self.active
                .as_ref()
                .ok_or(AdapterRuntimeError::Operation)?
                .commands
                .validate(revision, prompt.as_str())?;
        }
        if !self
            .capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.prompt)
            || self.delivery_unknown.is_some()
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        let blocks =
            crate::acp::attached_context::files::prompt_blocks(&prompt, self.capabilities.as_ref())
                .map_err(|_| AdapterRuntimeError::UnsupportedCommand)?;
        if self.close_pending {
            return Err(AdapterRuntimeError::Operation);
        }
        let id = self.next_request_id.saturating_add(1);
        let external_id = self
            .turn_external_id("prompt", id)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Operation)?;
        if active.turn.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        active
            .translator
            .begin_prompt(&id.to_string())
            .map_err(map_translation)?;
        active.turn = Some(TurnIntent {
            external_id,
            client_message_id,
            request_id: id,
            access_mode,
        });
        let session_id = active.external_id.clone();
        match self.reserve(
            PendingRequest::Prompt,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "session/prompt",
                "params": {
                    "sessionId": session_id.as_str(),
                    "prompt": blocks
                }
            }),
        ) {
            Ok(egress) => Ok(egress),
            Err(error) => {
                let active = self.active.as_mut().ok_or(AdapterRuntimeError::Operation)?;
                active
                    .translator
                    .reject_prompt(&id.to_string())
                    .map_err(map_translation)?;
                active.turn = None;
                Err(error)
            }
        }
    }

    /// Prompts in the profile's pinned session mode, switching to it first
    /// when the session is in another one. A profile with no pinned mode
    /// prompts in whatever mode the agent chose.
    pub(super) fn prompt_with_mode(
        &mut self,
        prompt: crate::acp::transcript_quote::Prompt,
        client_message_id: RequiredText<512>,
        access_mode: sage_acp_protocol::access_mode::SessionAccessMode,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        // The standard ACP access policy never admits these.
        if matches!(
            access_mode,
            sage_acp_protocol::access_mode::SessionAccessMode::AcceptEdits
                | sage_acp_protocol::access_mode::SessionAccessMode::Plan
                | sage_acp_protocol::access_mode::SessionAccessMode::Auto
                | sage_acp_protocol::access_mode::SessionAccessMode::FullAccess
        ) {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        let Some(mode_id) = self.profile.session_mode else {
            return self.prompt(prompt, client_message_id, access_mode);
        };
        if !self
            .available_modes
            .iter()
            .any(|available| available == mode_id)
            || self.pending_prompt.is_some()
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        if self.current_mode_id.as_deref() == Some(mode_id) {
            return self.prompt(prompt, client_message_id, access_mode);
        }
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Operation)?;
        if active.turn.is_some() || self.close_pending {
            return Err(AdapterRuntimeError::Operation);
        }
        let mode_id = mode_id.to_owned();
        let id = self.next_request_id.saturating_add(1);
        let egress = self.reserve(
            PendingRequest::SetMode,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "session/set_mode",
                "params": {"sessionId": active.external_id.as_str(), "modeId": mode_id}
            }),
        )?;
        self.pending_prompt = Some(DeferredPrompt {
            prompt,
            client_message_id,
            mode_id,
            request_id: id,
            cancelled: false,
            access_mode,
        });
        Ok(egress)
    }

    pub(super) fn cancel(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        if !self
            .capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.cancel)
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Operation)?;
        if active.turn.is_none() && self.pending_prompt.is_none() {
            return Err(AdapterRuntimeError::Operation);
        }
        if let Some(deferred) = self.pending_prompt.as_mut() {
            deferred.cancelled = true;
        }
        let frame = outbound(json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {"sessionId": active.external_id.as_str()}
        }))?;
        Ok(AdapterEgress::new(
            frame,
            crate::acp::bindings::adapter::EgressToken::cancel_notification(),
        ))
    }

    pub(super) fn close_session(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        if !self
            .capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.close_session)
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        if self.close_pending {
            return Err(AdapterRuntimeError::Operation);
        }
        let session_id = self
            .active
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?
            .external_id
            .clone();
        let id = self.next_request_id.saturating_add(1);
        let egress = self.reserve(
            PendingRequest::CloseSession,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "session/close",
                "params": {"sessionId": session_id.as_str()}
            }),
        )?;
        if let Some(deferred) = self.pending_prompt.as_mut() {
            deferred.cancelled = true;
        }
        self.close_pending = true;
        Ok(egress)
    }
}
