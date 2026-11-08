use super::*;

impl ClaudeRuntime {
    pub(super) fn command_inner(
        &mut self,
        command: AdapterCommand,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.delivery_unknown.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        match command {
            AdapterCommand::Steer { .. } => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::Initialize
                if self.capabilities.is_none()
                    && !self
                        .pending
                        .values()
                        .any(|pending| matches!(pending, PendingRequest::Initialize)) => self.reserve(
                PendingRequest::Initialize,
                "initialize",
                json!({"protocolVersion":1,"clientCapabilities":super::session_config::client_capabilities(),"clientInfo":{"name":"sage","version":env!("CARGO_PKG_VERSION")}}),
            ),
            AdapterCommand::Initialize => Err(AdapterRuntimeError::Operation),
            AdapterCommand::ModelPage { .. } => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::NewSession { native_root, model } => {
                self.require_capability(|value| value.new_session)?;
                if !self.accepts_default_model(model.as_ref()) || self.active.is_some() {
                    return Err(AdapterRuntimeError::UnsupportedCommand);
                }
                let root = RequiredText::<16_384>::new("nativeRoot", native_root)
                    .map_err(|_| AdapterRuntimeError::Operation)?;
                self.session_root = Some(root.as_str().to_owned());
                self.reserve(PendingRequest::NewSession, "session/new", json!({"cwd":root.as_str(),"mcpServers":self.mcp.wire(self.mcp_capabilities)}))
            }
            AdapterCommand::ResumeSession { external_session_id, native_root, model } => {
                self.require_capability(|value| value.resume_session)?;
                if !self.accepts_default_model(model.as_ref()) || self.active.is_some() {
                    return Err(AdapterRuntimeError::UnsupportedCommand);
                }
                let root = RequiredText::<16_384>::new("nativeRoot", native_root)
                    .map_err(|_| AdapterRuntimeError::Operation)?;
                self.session_root = Some(root.as_str().to_owned());
                self.reserve(PendingRequest::ResumeSession(external_session_id.clone()), "session/resume", json!({"sessionId":external_session_id.as_str(),"cwd":root.as_str(),"mcpServers":self.mcp.wire(self.mcp_capabilities)}))
            }
            AdapterCommand::LoadSession { .. } => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::CloseSession => {
                self.require_capability(|value| value.close_session)?;
                let session_id = self.active.as_ref().ok_or(AdapterRuntimeError::Operation)?.external_id.clone();
                self.reserve(PendingRequest::CloseSession, "session/close", json!({"sessionId":session_id.as_str()}))
            }
            AdapterCommand::Prompt {
                prompt,
                client_message_id,
                ..
            } => {
                self.require_capability(|value| value.prompt)?;
                let blocks = crate::acp::attached_context::files::prompt_blocks(&prompt, self.capabilities.as_ref()).map_err(|_| AdapterRuntimeError::UnsupportedCommand)?;
                if !self.auth.permits_prompt() {
                    return Err(AdapterRuntimeError::SignInRequired);
                }
                let id = self.next_request_id.checked_add(1).filter(|id| *id <= i64::MAX as u64).ok_or(AdapterRuntimeError::Operation)?;
                let active = self.active.as_mut().ok_or(AdapterRuntimeError::Operation)?;
                if active.turn.is_some() {
                    return Err(AdapterRuntimeError::Operation);
                }
                active.translator.begin_prompt(&id.to_string()).map_err(map_translation)?;
                active.turn = Some(TurnIntent {
                    external_turn_id: ExternalId::new("externalTurnId", format!("prompt-{id}"))
                        .map_err(|_| AdapterRuntimeError::Operation)?,
                    client_message_id,
                    request_id: id,
                    cancel_written: false,
                });
                let external_session_id = active.external_id.clone();
                match self.reserve(PendingRequest::Prompt, "session/prompt", json!({"sessionId":external_session_id.as_str(),"prompt":blocks})) {
                    Ok(egress) => Ok(egress),
                    Err(error) => {
                        let active = self
                            .active
                            .as_mut()
                            .ok_or(AdapterRuntimeError::Operation)?;
                        active.translator.reject_prompt(&id.to_string()).map_err(map_translation)?;
                        active.turn = None;
                        Err(error)
                    }
                }
            }
            AdapterCommand::Cancel => {
                self.require_capability(|value| value.cancel)?;
                let active = self.active.as_ref().ok_or(AdapterRuntimeError::Operation)?;
                if active.turn.is_none() {
                    return Err(AdapterRuntimeError::Operation);
                }
                cancel_notification(
                    "session/cancel",
                    json!({"sessionId":active.external_id.as_str()}),
                )
            }
            AdapterCommand::ResolvePermission {
                request_id,
                decision,
            } => {
                let record = self
                    .denials
                    .get_mut(request_id.as_str())
                    .ok_or(AdapterRuntimeError::LaterPermissionMiss)?;
                if record.state != DenialState::Pending {
                    return Err(AdapterRuntimeError::LaterPermissionMiss);
                }
                let reply = match decision {
                    PermissionDecision::Ask => {
                        let _ = resolve_later_permission(&request_id, decision, None)
                            .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?;
                        return Err(AdapterRuntimeError::Operation);
                    }
                    PermissionDecision::AllowOnce => {
                        let option_id = record.allow_once_option_id.as_deref().ok_or(AdapterRuntimeError::Operation)?;
                        let selected = permission_selected(&record.wire_id, option_id)?;
                        record.reply = PermissionReply::AllowOnce;
                        selected
                    }
                    PermissionDecision::AllowAlways => {
                        let option_id = record.allow_always_option_id.as_deref().ok_or(AdapterRuntimeError::Operation)?;
                        let selected = permission_selected(&record.wire_id, option_id)?;
                        record.reply = PermissionReply::AllowAlways;
                        selected
                    }
                    PermissionDecision::Deny => {
                        record.reply = PermissionReply::Deny;
                        let external = crate::acp::intent::ExternalId::new(
                            "permissionRequest",
                            request_id.as_str(),
                        )
                        .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?;
                        super::denial(&external, &record.wire_id, false)?
                    }
                };
                resolve_later_permission(&request_id, decision, Some(reply))
                    .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?
                    .ok_or(AdapterRuntimeError::LaterPermissionMiss)
            }
        }
    }
}

fn permission_selected(
    wire_id: &Value,
    option_id: &str,
) -> Result<AdapterEgress, AdapterRuntimeError> {
    let bytes = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": wire_id,
        "result": { "outcome": { "outcome": "selected", "optionId": option_id } },
    }))
    .map_err(|_| AdapterRuntimeError::Frame)?;
    let frame = OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
    Ok(AdapterEgress::new(
        frame,
        EgressToken::protocol_notification(),
    ))
}
