use super::{
    IngressError, ModelDescriptor, ReasoningEffort,
    support::{content, text},
};
use crate::acp::contract::RequiredText;
use sage_core::limits::LIMITS;
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
                RequiredText::<512>::new("defaultReasoningEffort", value.to_owned())
                    .map_err(content)
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
            supported_efforts,
            default_effort,
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
            Ok(ReasoningEffort {
                id: RequiredText::new(
                    "reasoningEffort",
                    text(effort, "reasoningEffort", 512)?.to_owned(),
                )
                .map_err(content)?,
                // Older and newer releases omit or blank the description.
                description: match effort.get("description") {
                    None | Some(Value::Null) => String::new(),
                    Some(Value::String(description))
                        if description.len() <= 8_192 && !description.contains('\0') =>
                    {
                        description.clone()
                    }
                    Some(_) => return Err(IngressError::Malformed),
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_reasoning_efforts;

    #[test]
    fn an_effort_with_new_members_or_no_description_is_still_offered() {
        let efforts = parse_reasoning_efforts(&json!([
            {"reasoningEffort":"high","description":"Deep","budget":"large"},
            {"reasoningEffort":"ultra"},
            {"reasoningEffort":"low","description":null}
        ]))
        .unwrap();
        let read = efforts
            .iter()
            .map(|effort| (effort.id.as_str(), effort.description.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(read, [("high", "Deep"), ("ultra", ""), ("low", "")]);
    }
}
