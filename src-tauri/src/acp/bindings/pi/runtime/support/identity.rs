use crate::acp::{
    bindings::adapter::{AdapterEvent, AdapterIdentity, AdapterModel, AdapterRuntimeError},
    contract::{RequiredText, SafeText},
    intent::{ExternalId, NormalizedPayload, ToolCategory},
    lifecycle::{ItemLifecycle, ToolState},
    limits::LIMITS,
};

use super::super::PiRuntime;
use crate::acp::bindings::pi::{ModelSummary, decode::redact_structured_text};

impl PiRuntime {
    pub(super) fn identity(
        &self,
        external_id: ExternalId,
        turn_id: Option<ExternalId>,
        client: Option<RequiredText<512>>,
    ) -> AdapterIdentity {
        AdapterIdentity {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch,
            session_id: self.session_id.clone(),
            external_id,
            turn_id,
            turn_client_message_id: client,
        }
    }

    pub(super) fn turn_identity(&self) -> Result<AdapterIdentity, AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?;
        Ok(self.identity(
            turn.external_id.clone(),
            Some(turn.external_id.clone()),
            Some(turn.client_message_id.clone()),
        ))
    }

    pub(super) fn item(
        &self,
        id: ExternalId,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?;
        Ok(AdapterEvent::Item {
            id: self.identity(
                id,
                Some(turn.external_id.clone()),
                Some(turn.client_message_id.clone()),
            ),
            lifecycle,
            tool_state,
            payload,
        })
    }

    pub(super) fn require_turn(&self, turn_key: &str) -> Result<(), AdapterRuntimeError> {
        match self.active_turn.as_ref() {
            Some(turn) if turn.external_id.as_str() == turn_key => Ok(()),
            _ => Err(AdapterRuntimeError::Operation),
        }
    }

    pub(super) fn next_local_item(
        &mut self,
        kind: &str,
    ) -> Result<ExternalId, AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        turn.next_item = turn
            .next_item
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(AdapterRuntimeError::Operation)?;
        external(
            "piItemId",
            format!("pi-{kind}-{}-{}", self.runtime_epoch.get(), turn.next_item),
        )
    }

    pub(super) fn start_text(
        &mut self,
        value: Option<String>,
    ) -> Result<ExternalId, AdapterRuntimeError> {
        let id = match value {
            Some(value) => external("piItemId", value)?,
            None => self.next_local_item("text")?,
        };
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        if turn.text_id.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        turn.text_id = Some(id.clone());
        turn.text_snapshot = Some(String::new());
        Ok(id)
    }

    pub(super) fn append_text(
        &mut self,
        delta: String,
    ) -> Result<(ExternalId, String), AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        let id = turn.text_id.clone().ok_or(AdapterRuntimeError::Operation)?;
        let current = turn
            .text_snapshot
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?;
        let length = current
            .len()
            .checked_add(delta.len())
            .ok_or(AdapterRuntimeError::BatchBytes)?;
        if length > LIMITS.wire.normalized_item_bytes as usize
            || length > LIMITS.wire.item_bytes_per_turn as usize
        {
            turn.text_snapshot = None;
            self.poisoned = true;
            return Err(AdapterRuntimeError::BatchBytes);
        }
        let mut next = String::new();
        if next.try_reserve_exact(length).is_err() {
            turn.text_snapshot = None;
            self.poisoned = true;
            return Err(AdapterRuntimeError::BatchBytes);
        }
        next.push_str(current);
        next.push_str(&delta);
        if current.ends_with('_') {
            remove_attached_continuation(current.len(), &mut next);
        }
        let next = redact_structured_text(&next);
        if next.len() > LIMITS.wire.normalized_item_bytes as usize
            || next.len() > LIMITS.wire.item_bytes_per_turn as usize
        {
            turn.text_snapshot = None;
            self.poisoned = true;
            return Err(AdapterRuntimeError::BatchBytes);
        }
        turn.text_snapshot = Some(next.clone());
        Ok((id, next))
    }

    pub(super) fn complete_text(
        &mut self,
        value: Option<String>,
    ) -> Result<(ExternalId, String), AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        let active = turn.text_id.clone().ok_or(AdapterRuntimeError::Operation)?;
        if let Some(value) = value
            && external("piItemId", value)? != active
        {
            return Err(AdapterRuntimeError::Operation);
        }
        let snapshot = turn
            .text_snapshot
            .take()
            .ok_or(AdapterRuntimeError::Operation)?;
        turn.text_id = None;
        Ok((active, snapshot))
    }
}

fn remove_attached_continuation(start: usize, joined: &mut String) {
    let suffix = &joined[start..];
    if suffix.starts_with(char::is_whitespace) {
        return;
    }
    let end = suffix
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index))
        .unwrap_or(suffix.len());
    joined.replace_range(start..start + end, "");
}

pub(super) fn model_event(
    models: Vec<ModelSummary>,
    current: Option<&str>,
) -> Result<AdapterEvent, AdapterRuntimeError> {
    if models.len() > LIMITS.wire.models_or_config_entries as usize {
        return Err(AdapterRuntimeError::BatchCount);
    }
    let mut bytes = 0_usize;
    for (index, model) in models.iter().enumerate() {
        let id = model_key(model)?;
        let name = if model.name.len() <= 512 {
            &model.name
        } else {
            &model.id
        };
        bytes = bytes
            .checked_add(id.len())
            .and_then(|value| value.checked_add(name.len()))
            .ok_or(AdapterRuntimeError::BatchBytes)?;
        if bytes > LIMITS.wire.models_or_config_bytes as usize
            || models[..index]
                .iter()
                .any(|other| model_key(other).ok().as_deref() == Some(&id))
        {
            return Err(AdapterRuntimeError::BatchBytes);
        }
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(models.len())
        .map_err(|_| AdapterRuntimeError::BatchBytes)?;
    for model in models {
        let id = model_key(&model)?;
        let is_default = current == Some(id.as_str());
        let name = if model.name.len() <= 512 {
            model.name
        } else {
            model.id.clone()
        };
        output.push(AdapterModel {
            id: RequiredText::new("modelId", id).map_err(|_| AdapterRuntimeError::Frame)?,
            name: RequiredText::new("modelName", name).map_err(|_| AdapterRuntimeError::Frame)?,
            is_default,
        });
    }
    Ok(AdapterEvent::Models {
        models: output,
        next_cursor: None,
    })
}

pub(super) fn external(
    field: &'static str,
    value: String,
) -> Result<ExternalId, AdapterRuntimeError> {
    ExternalId::new(field, value).map_err(|_| AdapterRuntimeError::Frame)
}

pub(super) fn text_payload(value: impl Into<String>) -> NormalizedPayload {
    NormalizedPayload::Text {
        text: SafeText::new("text", value).expect("Pi translator enforces text bound"),
    }
}

pub(super) fn tool_payload(
    label: impl Into<String>,
) -> Result<NormalizedPayload, AdapterRuntimeError> {
    let label = SafeText::new("toolLabel", label).map_err(|_| AdapterRuntimeError::Frame)?;
    Ok(NormalizedPayload::ToolCall {
        execution: Default::default(),
        category: ToolCategory::Other,
        label,
        input_summary: None,
        output_summary: None,
        locations: Default::default(),
        command_activity: Default::default(),
    })
}

pub(super) fn unsupported(kind: impl Into<String>, summary: impl Into<String>) -> AdapterEvent {
    AdapterEvent::Unsupported(NormalizedPayload::Unsupported {
        external_kind: SafeText::new("externalKind", kind).expect("Pi kind is bounded"),
        summary: SafeText::new("unsupportedSummary", summary).expect("fixed bounded summary"),
    })
}

pub(super) fn model_key(model: &ModelSummary) -> Result<String, AdapterRuntimeError> {
    let key = if model.provider.is_empty() {
        model.id.clone()
    } else {
        format!("{}:{}", model.provider, model.id)
    };
    if key.is_empty() || key.len() > 512 {
        return Err(AdapterRuntimeError::Frame);
    }
    Ok(key)
}
