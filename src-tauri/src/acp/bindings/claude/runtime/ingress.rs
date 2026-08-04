use super::*;

impl ClaudeRuntime {
    fn ingest_response(&mut self, value: Value) -> Result<AdapterBatch, AdapterRuntimeError> {
        let id = response_id(&value)?;
        let Some(pending) = self.pending.remove(&id) else {
            return self.ingest_late_prompt_response(value);
        };
        let result = match pending {
            PendingRequest::Initialize => self.initialize_response(value),
            PendingRequest::NewSession => self.session_response(value, None, false),
            PendingRequest::ResumeSession(expected) => {
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
        };
        if result.is_ok() && self.delivery_unknown == Some(DeliveryUnknown::Request(id)) {
            self.delivery_unknown = None;
        }
        result
    }

    fn initialize_response(&mut self, value: Value) -> Result<AdapterBatch, AdapterRuntimeError> {
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
                    id: model.id,
                    name: RequiredText::new("modelName", model.label.as_str())
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                    is_default: index == 0,
                })
            })
            .collect::<Result<Vec<_>, AdapterRuntimeError>>()?;
        capabilities.model_count = u16::from(!models.is_empty());
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
        let translator = ClaudeTranslator::new(external_id.as_str()).map_err(map_translation)?;
        self.active = Some(ActiveSession {
            external_id: external_id.clone(),
            translator,
            turn: None,
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
        if method != "session/update" {
            if value.get("id").is_some() {
                return Err(AdapterRuntimeError::MandatoryDenyTeardown);
            }
            return AdapterBatch::new(
                vec![SharedEvent::Unsupported(unsupported(method)?)],
                Vec::new(),
            );
        }
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        let events = active
            .translator
            .translate_notification_value(value, &MapperBridge(resources))
            .map_err(map_translation)?;
        self.translate_events(events)
    }

    fn permission_request(
        &mut self,
        value: Value,
        request_digest: [u8; 32],
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let (request_id, translator_id, wire_id) = permission_identity(value.get("id"))?;
        if let Some(record) = self
            .denials
            .get(request_id.as_str())
            .filter(|record| record.state == DenialState::Written)
        {
            return AdapterBatch::new(
                vec![SharedEvent::IgnoredDuplicate],
                vec![denial(&request_id, &record.wire_id, true)?],
            )
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown);
        }
        if self.denials.contains_key(request_id.as_str()) {
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let events = self
            .active
            .as_mut()
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?
            .translator
            .translate_permission_request_value(value)
            .map_err(map_translation)?;
        let active = self
            .active
            .as_ref()
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        let mut request_event = None;
        for event in events {
            if let AdapterEvent::Item { payload, .. } = event {
                request_event = Some(SharedEvent::PermissionRequest {
                    turn_id: self.turn_identity(active)?,
                    item_id: self.item_identity(active, request_id.clone())?,
                    payload,
                    request_payload_digest: request_digest,
                });
            }
        }
        let request_event = request_event.ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        let batch = AdapterBatch::new(
            vec![request_event],
            vec![denial(&request_id, &wire_id, false)?],
        )
        .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
        self.reserve_denial(request_id.as_str(), translator_id, wire_id)?;
        Ok(batch)
    }

    pub(super) fn reserve_denial(
        &mut self,
        request_id: &str,
        translator_id: String,
        wire_id: Value,
    ) -> Result<(), AdapterRuntimeError> {
        let limit = LIMITS.queues.server_requests_per_session as usize;
        if self.denials.len() >= limit {
            let oldest = self
                .denials
                .iter()
                .filter(|(_, record)| record.state == DenialState::Written)
                .min_by_key(|(_, record)| record.sequence)
                .map(|(id, _)| id.clone())
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
            self.denials.remove(&oldest);
        }
        let sequence = self
            .next_denial_sequence
            .checked_add(1)
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        self.denials.insert(
            request_id.to_owned(),
            DenialRecord {
                state: DenialState::Pending,
                sequence,
                translator_id,
                wire_id,
            },
        );
        self.next_denial_sequence = sequence;
        Ok(())
    }

    fn translate_events(
        &mut self,
        events: Vec<AdapterEvent>,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mut batch = AdapterBatchBuilder::new();
        let mut terminal = false;
        for event in events {
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
                AdapterEvent::TurnTerminal(state) => {
                    let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
                    batch.push_event(SharedEvent::Turn {
                        id: self.turn_identity(active)?,
                        state: terminal_state(state),
                    })?;
                    terminal = true;
                }
                AdapterEvent::IgnoredLate => batch.push_event(SharedEvent::IgnoredLate)?,
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

    fn item_identity(
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
