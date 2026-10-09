use super::{
    IngressError, ModelDescriptor, ReasoningEffort,
    support::{content, text},
};
use crate::acp::{contract::RequiredText, limits::LIMITS};
use serde_json::{Map, Value};

pub(super) fn parse_models(
    result: &Map<String, Value>,
) -> Result<(Vec<ModelDescriptor>, Option<RequiredText<512>>), IngressError> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or(IngressError::Malformed)?;
    if data.len() > LIMITS.wire.models_or_config_entries as usize {
        return Err(IngressError::CollectionLimit);
    }
    let mut total = 0_usize;
    let mut models = Vec::with_capacity(data.len());
    for value in data {
        let model = value.as_object().ok_or(IngressError::Malformed)?;
        let id = text(model, "id", 512)?;
        let name = text(model, "displayName", 512)?;
        let default_effort = model
            .get("defaultReasoningEffort")
            .and_then(Value::as_str)
            .map(|value| {
                RequiredText::new("defaultReasoningEffort", value.to_owned()).map_err(content)
            })
            .transpose()?;
        let supported_efforts = model
            .get("supportedReasoningEfforts")
            .map(parse_reasoning_efforts)
            .transpose()?
            .unwrap_or_default();
        total = total
            .saturating_add(id.len())
            .saturating_add(name.len())
            .saturating_add(
                default_effort
                    .as_ref()
                    .map_or(0, |value| value.as_str().len()),
            );
        for effort in &supported_efforts {
            total = total
                .saturating_add(effort.id.as_str().len())
                .saturating_add(effort.description.len());
        }
        if total > LIMITS.wire.models_or_config_bytes as usize
            || models
                .iter()
                .any(|existing: &ModelDescriptor| existing.id.as_str() == id)
        {
            return Err(IngressError::CollectionLimit);
        }
        models.push(ModelDescriptor {
            id: RequiredText::new("modelId", id.to_owned()).map_err(content)?,
            name: RequiredText::new("modelName", name.to_owned()).map_err(content)?,
            is_default: model
                .get("isDefault")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            default_effort,
            supported_efforts,
        });
    }
    let next_cursor = result
        .get("nextCursor")
        .filter(|value| !value.is_null())
        .map(|value| {
            let cursor = value.as_str().ok_or(IngressError::Malformed)?;
            if cursor.is_empty() || cursor.len() > LIMITS.wire.page_cursor_bytes as usize {
                return Err(IngressError::CollectionLimit);
            }
            RequiredText::new("modelCursor", cursor.to_owned()).map_err(content)
        })
        .transpose()?;
    Ok((models, next_cursor))
}

pub(in crate::acp::bindings::codex) fn parse_reasoning_efforts(
    value: &Value,
) -> Result<Vec<ReasoningEffort>, IngressError> {
    let values = value.as_array().ok_or(IngressError::Malformed)?;
    if values.len() > LIMITS.codex.reasoning_efforts_per_model as usize {
        return Err(IngressError::CollectionLimit);
    }
    values
        .iter()
        .map(|value| {
            let effort = value.as_object().ok_or(IngressError::Malformed)?;
            if effort.len() != 2 {
                return Err(IngressError::Malformed);
            }
            Ok(ReasoningEffort {
                id: RequiredText::new(
                    "reasoningEffort",
                    text(effort, "reasoningEffort", 512)?.to_owned(),
                )
                .map_err(content)?,
                description: text(effort, "description", 8_192)?.to_owned(),
            })
        })
        .collect()
}
