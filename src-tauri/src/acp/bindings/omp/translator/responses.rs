use std::hash::{DefaultHasher, Hash, Hasher};

use serde_json::{Map, Value};

use super::{
    CompletedResponse, MAX_COMPLETED_REQUESTS, OmpTranslator, PendingKind, TranslationError,
    validated_id,
};
use crate::acp::bindings::omp::{
    protocol::{AdapterEvent, ModelSummary, PromptTerminal},
    wire::{normalized_text, opaque_id},
};

const MAX_MODELS: usize = 128;
const MAX_MODEL_BYTES: usize = 262_144;

impl OmpTranslator {
    pub(super) fn translate_response(
        &mut self,
        frame: &Value,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let request_id = opaque_id(frame.get("id"))?;
        let fingerprint = response_fingerprint(frame)?;
        let Some(kind) = self.pending.remove(&request_id) else {
            return match self.completed.get(&request_id) {
                Some(completed) if completed.fingerprint == fingerprint => {
                    Ok(vec![AdapterEvent::IgnoredDuplicate])
                }
                Some(_) => Err(TranslationError::TerminalConflict),
                None => Err(TranslationError::Correlation),
            };
        };
        self.remember_response(request_id.clone(), fingerprint);
        if frame.get("error").is_some() {
            if kind == PendingKind::Prompt {
                return self.finish_turn(PromptTerminal::Failed);
            }
            return Err(TranslationError::RemoteFailure);
        }
        let result = frame
            .get("result")
            .and_then(Value::as_object)
            .ok_or(TranslationError::MalformedFrame)?;
        match kind {
            PendingKind::Initialize => self.finish_initialize(result),
            PendingKind::NewSession => self.finish_session(result, None),
            PendingKind::LoadSession { expected_session } => {
                self.finish_session(result, Some(&expected_session))
            }
            PendingKind::Prompt => self.finish_prompt(&request_id, result),
        }
    }

    fn finish_initialize(
        &mut self,
        result: &Map<String, Value>,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let protocol_version = result
            .get("protocolVersion")
            .and_then(Value::as_u64)
            .ok_or(TranslationError::MalformedFrame)?;
        let capabilities = result
            .get("agentCapabilities")
            .and_then(Value::as_object)
            .ok_or(TranslationError::MalformedFrame)?;
        if protocol_version != 1 {
            return Err(TranslationError::UnsupportedOperation("protocol version"));
        }
        self.negotiated_load = capabilities
            .get("loadSession")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && self.capabilities.load_session_ceiling();
        self.initialized = true;
        Ok(vec![AdapterEvent::Initialized {
            protocol_version,
            load_session: self.negotiated_load,
        }])
    }

    fn finish_session(
        &mut self,
        result: &Map<String, Value>,
        expected_session: Option<&str>,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let session_id = validated_id("externalSessionId", opaque_id(result.get("sessionId"))?)?;
        if expected_session.is_some_and(|expected| expected != session_id.as_str()) {
            return Err(TranslationError::Correlation);
        }
        let models = parse_models(result.get("models"))?;
        self.session_id = Some(session_id.clone());
        self.active = None;
        Ok(vec![AdapterEvent::SessionReady {
            external_session_id: session_id,
            models,
        }])
    }

    fn finish_prompt(
        &mut self,
        request_id: &str,
        result: &Map<String, Value>,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let active = self.active.as_ref().ok_or(TranslationError::StaleIntent)?;
        if active.request_id != request_id {
            return Err(TranslationError::Correlation);
        }
        let terminal = match result.get("stopReason").and_then(Value::as_str) {
            Some("end_turn") => PromptTerminal::Completed,
            Some("cancelled") => PromptTerminal::Cancelled,
            Some("refusal" | "max_tokens" | "max_turn_requests") => PromptTerminal::Failed,
            _ => return Err(TranslationError::UnknownTerminal),
        };
        self.finish_turn(terminal)
    }

    pub(super) fn finish_turn(
        &mut self,
        terminal: PromptTerminal,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let active = self.active.as_mut().ok_or(TranslationError::StaleIntent)?;
        match active.terminal {
            Some(current) if current == terminal => Ok(vec![AdapterEvent::IgnoredDuplicate]),
            Some(_) => Err(TranslationError::TerminalConflict),
            None => {
                active.terminal = Some(terminal);
                active.tools.clear();
                active.tool_categories.clear();
                active.command_snapshots.clear();
                active.command_snapshot_bytes = 0;
                Ok(vec![AdapterEvent::TurnTerminal {
                    turn_key: active.turn_key.clone(),
                    terminal,
                }])
            }
        }
    }

    fn remember_response(&mut self, id: String, fingerprint: u64) {
        if self.completed.len() == MAX_COMPLETED_REQUESTS
            && let Some(oldest) = self.completed_order.pop_front()
        {
            self.completed.remove(&oldest);
        }
        self.completed
            .insert(id.clone(), CompletedResponse { fingerprint });
        self.completed_order.push_back(id);
    }
}

fn parse_models(value: Option<&Value>) -> Result<Vec<ModelSummary>, TranslationError> {
    let Some(models) = value else {
        return Ok(Vec::new());
    };
    let object = models.as_object().ok_or(TranslationError::MalformedFrame)?;
    let current = object.get("currentModelId").and_then(Value::as_str);
    let available = object
        .get("availableModels")
        .and_then(Value::as_array)
        .ok_or(TranslationError::MalformedFrame)?;
    if available.len() > MAX_MODELS {
        return Err(TranslationError::Limit("model count"));
    }
    let mut total = 0_usize;
    let models = available
        .iter()
        .map(|model| {
            let id = validated_id("modelId", opaque_id(model.get("modelId"))?)?;
            let raw_name = model
                .get("name")
                .and_then(Value::as_str)
                .ok_or(TranslationError::MalformedFrame)?;
            total = total
                .checked_add(id.as_str().len())
                .and_then(|value| value.checked_add(raw_name.len()))
                .ok_or(TranslationError::Limit("model bytes"))?;
            if total > MAX_MODEL_BYTES {
                return Err(TranslationError::Limit("model bytes"));
            }
            Ok(ModelSummary {
                is_current: current == Some(id.as_str()),
                external_id: id,
                name: normalized_text::<512>("modelName", raw_name)?
                    .as_str()
                    .to_owned(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (index, model) in models.iter().enumerate() {
        if models[..index]
            .iter()
            .any(|other| other.external_id == model.external_id)
        {
            return Err(TranslationError::Correlation);
        }
    }
    if current.is_some() && models.iter().filter(|model| model.is_current).count() != 1 {
        return Err(TranslationError::Correlation);
    }
    Ok(models)
}

fn response_fingerprint(value: &Value) -> Result<u64, TranslationError> {
    let encoded = serde_json::to_string(value).map_err(|_| TranslationError::MalformedFrame)?;
    let mut hasher = DefaultHasher::new();
    encoded.hash(&mut hasher);
    Ok(hasher.finish())
}
