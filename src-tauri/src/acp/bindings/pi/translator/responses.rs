use super::*;

impl PiTranslator {
    pub(super) fn ingest_response(
        &mut self,
        response: Response,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        let fingerprint = response_fingerprint(&response);
        let Some(kind) = self.pending.remove(&response.id) else {
            return self.classify_repeated_response(&response, fingerprint);
        };
        let closed = self.closed_config_requests.remove(&response.id)
            || (kind.is_config()
                && self
                    .active
                    .as_ref()
                    .is_some_and(|turn| turn.authority_denied));
        if response.command != kind.command() {
            if kind == PendingKind::SetThinkingLevel {
                self.pending_thinking = None;
            }
            if matches!(kind, PendingKind::State | PendingKind::Models) {
                self.reset_initialization();
            }
            return Err(AdapterError::Correlation);
        }
        self.remember_response(&response, fingerprint);
        if closed {
            if kind == PendingKind::SetThinkingLevel {
                self.pending_thinking = None;
            }
            return Ok(vec![AdapterOutput::LateIgnored]);
        }
        if !response.success {
            if kind == PendingKind::SetThinkingLevel {
                self.pending_thinking = None;
            }
            if matches!(kind, PendingKind::SetModel | PendingKind::SetThinkingLevel) {
                return Err(AdapterError::ConfigMutationRejected);
            }
            let mut output = vec![AdapterOutput::Error {
                code: "command_failed",
            }];
            if kind == PendingKind::Prompt {
                output.extend(self.finish_turn(TerminalOutcome::Failed)?);
            } else if kind == PendingKind::Abort {
                let active = self.active.as_mut().ok_or(AdapterError::StaleIntent)?;
                active.abort_requested = false;
            } else if matches!(kind, PendingKind::State | PendingKind::Models) {
                self.reset_initialization();
            }
            return Ok(output);
        }
        match kind {
            PendingKind::State => {
                let state = match parse_state(&response.data) {
                    Ok(state) => state,
                    Err(error) => {
                        self.reset_initialization();
                        return Err(error);
                    }
                };
                let mut output = vec![AdapterOutput::State(state)];
                self.note_initialization(&response.id, PendingKind::State, &mut output);
                Ok(output)
            }
            PendingKind::Models => {
                let models = match parse_models(&response.data) {
                    Ok(models) => models,
                    Err(error) => {
                        self.reset_initialization();
                        return Err(error);
                    }
                };
                let mut output = vec![AdapterOutput::Models(models)];
                self.note_initialization(&response.id, PendingKind::Models, &mut output);
                Ok(output)
            }
            PendingKind::SetModel => Ok(vec![AdapterOutput::ModelSelected(
                self::payload::parse_model(&response.data)?,
            )]),
            PendingKind::ThinkingLevels => Ok(vec![AdapterOutput::ThinkingLevels(
                self::payload::parse_thinking_levels(&response.data)?,
            )]),
            PendingKind::SetThinkingLevel => {
                let (id, level) = self
                    .pending_thinking
                    .take()
                    .ok_or(AdapterError::Correlation)?;
                if id != response.id {
                    return Err(AdapterError::Correlation);
                }
                Ok(vec![AdapterOutput::ThinkingSelected(level)])
            }
            PendingKind::NewSession => {
                self.active = None;
                Ok(vec![AdapterOutput::SessionReset])
            }
            PendingKind::Prompt => {
                let active = self.active.as_mut().ok_or(AdapterError::StaleIntent)?;
                if active.prompt_command_id != response.id {
                    return Err(AdapterError::Correlation);
                }
                if active.terminal.is_some() {
                    return Ok(vec![AdapterOutput::LateIgnored]);
                }
                if active.event_phase != EventPhase::PromptIssued {
                    return Err(AdapterError::EventSequence);
                }
                active.event_phase = EventPhase::PromptAccepted;
                Ok(vec![AdapterOutput::CommandAccepted { command: "prompt" }])
            }
            PendingKind::Abort => Ok(vec![AdapterOutput::CommandAccepted { command: "abort" }]),
            PendingKind::Messages => Ok(vec![AdapterOutput::Messages(parse_messages(
                &response.data,
            )?)]),
            PendingKind::Entries => {
                Ok(vec![AdapterOutput::Entries(parse_entries(&response.data)?)])
            }
        }
    }

    fn classify_repeated_response(
        &self,
        response: &Response,
        fingerprint: u64,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        match self.completed.get(&response.id) {
            Some(completed)
                if completed.command == response.command
                    && completed.success == response.success
                    && completed.payload_fingerprint == fingerprint =>
            {
                let closed = self
                    .active
                    .as_ref()
                    .is_some_and(|turn| turn.terminal.is_some() || turn.authority_denied)
                    && matches!(
                        response.command.as_str(),
                        "set_model"
                            | "set_thinking_level"
                            | "get_available_thinking_levels"
                            | "get_state"
                    );
                Ok(vec![if closed {
                    AdapterOutput::LateIgnored
                } else {
                    AdapterOutput::DuplicateIgnored
                }])
            }
            Some(_) => Err(AdapterError::Correlation),
            None => Err(AdapterError::Correlation),
        }
    }

    fn remember_response(&mut self, response: &Response, fingerprint: u64) {
        if self.completed.len() == COMPLETED_COMMANDS
            && let Some(oldest) = self.completed_order.pop_front()
        {
            self.completed.remove(&oldest);
        }
        self.completed.insert(
            response.id.clone(),
            CompletedResponse {
                command: response.command.clone(),
                success: response.success,
                payload_fingerprint: fingerprint,
            },
        );
        self.completed_order.push_back(response.id.clone());
    }
}
