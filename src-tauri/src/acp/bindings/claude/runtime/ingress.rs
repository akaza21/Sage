use super::*;

mod permission;
mod read_text_file;

impl ClaudeRuntime {
    fn binding_is_characterised(&self) -> bool {
        self.launch_binding
            .as_ref()
            .is_none_or(|binding| binding.characterisation().is_characterised())
    }

    fn ingest_response(&mut self, value: Value) -> Result<AdapterBatch, AdapterRuntimeError> {
        let id = response_id(&value)?;
        let Some(pending) = self.pending.remove(&id) else {
            return self.ingest_late_prompt_response(value);
        };
        if let Some(observed) = decode_auth_required_error(&value) {
            self.auth = observed;
            if matches!(
                pending,
                PendingRequest::Prompt
                    | PendingRequest::NewSession
                    | PendingRequest::ResumeSession(_)
            ) {
                self.refusal_detail = profile_for(CLAUDE_PROFILE_ID)
                    .and_then(|profile| auth_refusal_detail(profile, &value));
                if matches!(pending, PendingRequest::Prompt) {
                    if let Some(active) = self.active.as_mut() {
                        active.turn = None;
                        active
                            .translator
                            .reject_prompt(&id.to_string())
                            .map_err(map_translation)?;
                    }
                    self.deferred.clear();
                    if self.delivery_unknown == Some(DeliveryUnknown::Request(id)) {
                        self.delivery_unknown = None;
                    }
                }
                return Err(AdapterRuntimeError::SignInRequired);
            }
        }
        let result = match pending {
            PendingRequest::Initialize => self.initialize_response(value),
            PendingRequest::NewSession => self.session_response(value, None, false),
            PendingRequest::ResumeSession(expected) => {
                if value.get("error").is_some() {
                    let error = &value["error"];
                    if error.get("code").and_then(Value::as_i64).is_none()
                        || error.get("message").and_then(Value::as_str).is_none()
                        || value.get("result").is_some()
                        || value.get("method").is_some()
                    {
                        return Err(AdapterRuntimeError::Frame);
                    }
                    // Only this binding's definitive missing-session response permits
                    // discarding continuation. Authentication and server failures do not.
                    return Err(
                        if self.binding_is_characterised()
                            && error["code"].as_i64() == Some(-32603)
                            && error["message"].as_str() == Some("Internal error")
                            && error.pointer("/data/details").and_then(Value::as_str)
                                == Some("Session not found")
                        {
                            AdapterRuntimeError::ResumeRefused
                        } else {
                            AdapterRuntimeError::Operation
                        },
                    );
                }
                self.session_response(value, Some(expected), true)
            }
            PendingRequest::CloseSession => {
                if value.get("error").is_some()
                    || !value.get("result").is_some_and(Value::is_object)
                {
                    return Err(AdapterRuntimeError::Frame);
                }
                self.active = None;
                AdapterBatch::new(vec![SharedEvent::Acknowledged], Vec::new())
            }
            PendingRequest::Prompt => self.prompt_response(value),
            PendingRequest::SetMode => self.config_response(value, false),
            PendingRequest::SetConfig => self.config_response(value, true),
            PendingRequest::Fork(source) => self.fork_response(&value, &source),
            PendingRequest::DeleteFork(background, _) => {
                self.delete_fork_response(&value, background)
            }
        };
        if result.is_ok() && self.delivery_unknown == Some(DeliveryUnknown::Request(id)) {
            self.delivery_unknown = None;
        }
        result
    }

    fn initialize_response(&mut self, value: Value) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.mcp_capabilities = McpCapabilities::from_initialize(&value);
        let offer = ClaudeCapabilityOffer::from_initialize_value(&value).map_err(map_capability)?;
        let mut capabilities = negotiate_capabilities(&offer).map_err(map_capability)?;
        if offer.models.len() as u64 > LIMITS.wire.models_or_config_entries
            || model_bytes(&offer.models)? > LIMITS.wire.models_or_config_bytes as usize
        {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        let models = offer
            .models
            .into_iter()
            .take(1)
            .enumerate()
            .map(|(index, model)| {
                Ok(AdapterModel {
                    context_limits: model.context_limits,
                    id: model.id,
                    name: RequiredText::new("modelName", model.label.as_str())
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                    is_default: index == 0,
                })
            })
            .collect::<Result<Vec<_>, AdapterRuntimeError>>()?;
        capabilities.model_count = u16::from(!models.is_empty());
        if !self.binding_is_characterised() {
            capabilities.resume_session = false;
            capabilities.native_session_fork = false;
            capabilities.child_conversation_projection = false;
            capabilities.collaboration_event_projection = false;
        }
        // `session/load` requires cwd/MCP context that the frozen shared command
        // does not carry. Intersect the source offer with Sage's safe surface.
        capabilities.load_session = false;
        let default_model = models.first().map(|model| model.id.clone());
        let mut batch = AdapterBatchBuilder::new();
        batch.push_event(SharedEvent::Initialized {
            capabilities: capabilities.clone(),
        })?;
        if !models.is_empty() {
            batch.push_event(SharedEvent::Models {
                models,
                next_cursor: None,
            })?;
        }
        self.default_model = default_model;
        self.capabilities = Some(capabilities);
        Ok(batch.finish())
    }

    fn session_response(
        &mut self,
        value: Value,
        expected: Option<ExternalId>,
        loaded: bool,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let external_id = result_session_id(&value)?;
        if expected
            .as_ref()
            .is_some_and(|expected| expected != &external_id)
        {
            return Err(AdapterRuntimeError::Frame);
        }
        let translator =
            ClaudeTranslator::new(super::super::claude_profile(), external_id.as_str())
                .map_err(map_translation)?;
        self.active = Some(ActiveSession {
            external_id: external_id.clone(),
            translator,
            turn: None,
            config: crate::acp::session_config::SessionConfig::from_wire(&value["result"], true)
                .map_err(|_| AdapterRuntimeError::Frame)?,
        });
        AdapterBatch::new(
            vec![SharedEvent::SessionReady {
                id: self.session_identity(external_id),
                loaded,
            }],
            Vec::new(),
        )
    }

    fn ingest_late_prompt_response(
        &mut self,
        value: Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        let events = active
            .translator
            .translate_prompt_result_value(value)
            .map_err(map_translation)?;
        self.translate_events(events)
    }

    fn prompt_response(&mut self, value: Value) -> Result<AdapterBatch, AdapterRuntimeError> {
        let request_id = response_id(&value)?;
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        if active.turn.as_ref().map(|turn| turn.request_id) != Some(request_id) {
            return Err(AdapterRuntimeError::Frame);
        }
        let events = active
            .translator
            .translate_prompt_result_value(value)
            .map_err(map_translation)?;
        self.translate_events(events)
    }

    fn ingest_notification(
        &mut self,
        value: Value,
        resources: &dyn SharedResourceMapper,
        request_digest: [u8; 32],
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .ok_or(AdapterRuntimeError::Frame)?;
        if method == "session/request_permission" {
            return self.permission_request(value, request_digest);
        }
        if method == crate::acp::runtime::host_browse::CURATED_BROWSE_TOOL_NAME {
            return self.ingest_browse_host(value);
        }
        if value.get("id").is_none()
            && let Some(observed) = decode_auth_status_update(&value)
        {
            self.auth = observed;
            return AdapterBatch::new(vec![SharedEvent::Absorbed], Vec::new());
        }
        if method != "session/update" {
            if value.get("id").is_some() {
                return Err(AdapterRuntimeError::MandatoryDenyTeardown);
            }
            return AdapterBatch::new(
                vec![SharedEvent::Unsupported(unsupported(method)?)],
                Vec::new(),
            );
        }
        if self.absorbs_fork_update(&value) {
            return AdapterBatch::new(vec![SharedEvent::Absorbed], Vec::new());
        }
        if self.config_notification(&value)? {
            return AdapterBatch::new(vec![SharedEvent::IgnoredDuplicate], Vec::new());
        }
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        let events = active
            .translator
            .translate_notification_value(value, &MapperBridge(resources))
            .map_err(map_translation)?;
        self.translate_events(events)
    }

    fn translate_events(
        &mut self,
        events: Vec<AdapterEvent>,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mut batch = AdapterBatchBuilder::new();
        let mut terminal = false;
        let deferred = std::mem::take(&mut self.deferred);
        for event in deferred.into_iter().chain(events) {
            match event {
                AdapterEvent::Item {
                    external_key,
                    lifecycle,
                    tool_state,
                    payload,
                } => {
                    let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
                    batch.push_event(SharedEvent::Item {
                        id: self.item_identity(active, external_key)?,
                        lifecycle,
                        tool_state,
                        payload,
                    })?;
                }
                AdapterEvent::FilePair {
                    external_key,
                    old_text,
                    new_text,
                } => {
                    let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
                    batch.push_event(SharedEvent::FilePair {
                        id: self.item_identity(active, external_key.clone())?,
                        external_item_id: external_key,
                        old_text,
                        new_text,
                    })?;
                }
                AdapterEvent::TurnTerminal(state) => {
                    let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
                    batch.push_event(SharedEvent::Turn {
                        id: self.turn_identity(active)?,
                        state: terminal_state(state),
                    })?;
                    terminal = true;
                }
                AdapterEvent::TurnRunning => {
                    let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
                    batch.push_event(SharedEvent::Turn {
                        id: self.turn_identity(active)?,
                        state: TurnState::Running,
                    })?;
                }
                AdapterEvent::Usage(payload) => {
                    let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
                    batch.push_event(SharedEvent::Usage {
                        turn_id: self.turn_identity(active)?,
                        payload,
                    })?;
                }
                AdapterEvent::IgnoredLate => batch.push_event(SharedEvent::IgnoredLate)?,
                AdapterEvent::Absorbed => batch.push_event(SharedEvent::Absorbed)?,
                AdapterEvent::IgnoredDuplicate => {
                    batch.push_event(SharedEvent::IgnoredDuplicate)?
                }
                AdapterEvent::PermissionDecline { .. } => {
                    return Err(AdapterRuntimeError::MandatoryDenyTeardown);
                }
            }
        }
        if terminal && let Some(active) = self.active.as_mut() {
            active.turn = None;
        }
        Ok(batch.finish())
    }

    fn session_identity(
        &self,
        external_id: ExternalId,
    ) -> crate::acp::bindings::adapter::AdapterIdentity {
        identity(
            self.binding_digest,
            self.runtime_epoch,
            &self.session_id,
            external_id,
            None,
            None,
        )
    }

    fn turn_identity(
        &self,
        active: &ActiveSession,
    ) -> Result<crate::acp::bindings::adapter::AdapterIdentity, AdapterRuntimeError> {
        let turn = active.turn.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        Ok(identity(
            self.binding_digest,
            self.runtime_epoch,
            &self.session_id,
            active.external_id.clone(),
            Some(turn.external_turn_id.clone()),
            Some(turn.client_message_id.clone()),
        ))
    }

    pub(super) fn item_identity(
        &self,
        active: &ActiveSession,
        external_id: ExternalId,
    ) -> Result<crate::acp::bindings::adapter::AdapterIdentity, AdapterRuntimeError> {
        let turn = active.turn.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        Ok(identity(
            self.binding_digest,
            self.runtime_epoch,
            &self.session_id,
            external_id,
            Some(turn.external_turn_id.clone()),
            Some(turn.client_message_id.clone()),
        ))
    }

    pub(super) fn ingest_inner(
        &mut self,
        frame: &Frame,
        resources: &dyn SharedResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let value = decode_frame(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        if value.get("method").is_some() {
            let request_digest =
                request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame)?;
            self.ingest_notification(value, resources, request_digest)
        } else {
            self.ingest_response(value)
        }
    }
}

pub(super) fn terminal_state(value: PromptTerminal) -> TurnState {
    match value {
        PromptTerminal::Completed => TurnState::Completed,
        PromptTerminal::Cancelled => TurnState::Cancelled,
        PromptTerminal::Failed => TurnState::Failed,
    }
}
