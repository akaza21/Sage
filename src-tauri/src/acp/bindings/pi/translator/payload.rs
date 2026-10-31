use serde_json::Value;

use crate::acp::bindings::pi::{
    decode::{
        EXTERNAL_ID_BYTES, ITEM_BYTES, MODEL_BYTES, MODEL_ENTRIES, MODEL_NAME_BYTES, PAGE_BYTES,
        PAGE_ENTRIES, bounded_string, external_id, opaque_id, redact_structured_text,
        validate_opaque_text,
    },
    protocol::{
        AdapterError, EntrySummary, HistoryMessage, HistoryRole, ModelSummary, StateSnapshot,
    },
};

pub(super) fn parse_models(value: &Value) -> Result<Vec<ModelSummary>, AdapterError> {
    let models = value.get("models").unwrap_or(value);
    let models = models.as_array().ok_or(AdapterError::Malformed)?;
    if models.len() > MODEL_ENTRIES {
        return Err(AdapterError::Limit("model count"));
    }
    preflight_models(models)?;
    models.iter().map(parse_model).collect()
}

pub(super) fn parse_model(model: &Value) -> Result<ModelSummary, AdapterError> {
    let provider = opaque_id(model.get("provider"), 512)?.unwrap_or_default();
    let id = opaque_id(model.get("id").or_else(|| model.get("modelId")), 512)?
        .ok_or(AdapterError::Malformed)?;
    let name = bounded_string(model.get("name"), MODEL_NAME_BYTES)?.unwrap_or_else(|| id.clone());
    Ok(ModelSummary {
        provider,
        id,
        name,
        context_limits: crate::acp::context_handoff::budget::ModelContextLimits::from_model(model),
    })
}

pub(super) fn parse_thinking_levels(value: &Value) -> Result<Vec<String>, AdapterError> {
    let levels = value
        .get("levels")
        .and_then(Value::as_array)
        .ok_or(AdapterError::Malformed)?;
    if levels.len() > 7 {
        return Err(AdapterError::Limit("thinking levels"));
    }
    let mut output = Vec::new();
    for level in levels {
        let level = level.as_str().ok_or(AdapterError::Malformed)?;
        if !super::valid_thinking_level(level) || output.iter().any(|other| other == level) {
            return Err(AdapterError::Malformed);
        }
        output.push(level.to_owned());
    }
    Ok(output)
}

pub(super) fn parse_state(value: &Value) -> Result<StateSnapshot, AdapterError> {
    let model = value
        .get("model")
        .and_then(Value::as_object)
        .map(|model| {
            let provider = opaque_id(model.get("provider"), 512)?.unwrap_or_default();
            let id = opaque_id(model.get("id").or_else(|| model.get("modelId")), 512)?
                .ok_or(AdapterError::Malformed)?;
            let name =
                bounded_string(model.get("name"), MODEL_NAME_BYTES)?.unwrap_or_else(|| id.clone());
            Ok(ModelSummary {
                provider,
                id,
                name,
                context_limits: crate::acp::context_handoff::budget::ModelContextLimits::from_model(
                    &Value::Object(model.clone()),
                ),
            })
        })
        .transpose()?;
    Ok(StateSnapshot {
        model,
        thinking_level: bounded_string(
            value
                .get("thinkingLevel")
                .or_else(|| value.get("thinking_level")),
            128,
        )?,
        has_session: value
            .get("sessionFile")
            .or_else(|| value.get("session_file"))
            .and_then(Value::as_str)
            .is_some(),
        is_streaming: value
            .get("isStreaming")
            .or_else(|| value.get("is_streaming"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

pub(super) fn parse_messages(value: &Value) -> Result<Vec<HistoryMessage>, AdapterError> {
    let messages = value.get("messages").unwrap_or(value);
    let messages = messages.as_array().ok_or(AdapterError::Malformed)?;
    if messages.len() > PAGE_ENTRIES {
        return Err(AdapterError::Limit("message count"));
    }
    preflight_messages(messages)?;
    messages
        .iter()
        .filter_map(|message| match parse_message(message) {
            Ok(Some(message)) => Some(Ok(message)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

fn parse_message(message: &Value) -> Result<Option<HistoryMessage>, AdapterError> {
    let role = match message.get("role").and_then(Value::as_str) {
        Some("user") => HistoryRole::User,
        Some("assistant") => HistoryRole::Assistant,
        Some("toolResult" | "tool") => HistoryRole::Tool,
        _ => return Ok(None),
    };
    let content = message.get("content").ok_or(AdapterError::Malformed)?;
    let mut text = String::new();
    match content {
        Value::String(value) => text.push_str(value),
        Value::Array(parts) => {
            for part in parts {
                if part.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(value) = part.get("text").and_then(Value::as_str)
                {
                    text.push_str(value);
                }
            }
        }
        _ => return Err(AdapterError::Malformed),
    }
    Ok(Some(HistoryMessage {
        role,
        text: redact_structured_text(&text),
    }))
}

pub(super) fn parse_entries(value: &Value) -> Result<Vec<EntrySummary>, AdapterError> {
    let entries = value.get("entries").unwrap_or(value);
    let entries = entries.as_array().ok_or(AdapterError::Malformed)?;
    if entries.len() > PAGE_ENTRIES {
        return Err(AdapterError::Limit("entry count"));
    }
    preflight_entries(entries)?;
    entries
        .iter()
        .map(|entry| {
            let external_id = external_id(entry.get("id"))?.ok_or(AdapterError::Malformed)?;
            let kind = bounded_string(entry.get("type"), 128)?.ok_or(AdapterError::Malformed)?;
            Ok(EntrySummary { external_id, kind })
        })
        .collect()
}

fn preflight_models(models: &[Value]) -> Result<(), AdapterError> {
    let mut total = 0_usize;
    for model in models {
        let provider = optional_opaque(model.get("provider"))?;
        let id = required_opaque(model.get("id").or_else(|| model.get("modelId")))?;
        let name = model
            .get("name")
            .map(|value| required_text(Some(value)))
            .transpose()?
            .unwrap_or(id);
        if name.len() > MODEL_NAME_BYTES {
            return Err(AdapterError::Limit("text bytes"));
        }
        total = total
            .checked_add(provider.len())
            .and_then(|value| value.checked_add(id.len()))
            .and_then(|value| value.checked_add(name.len()))
            .ok_or(AdapterError::Limit("model bytes"))?;
        if total > MODEL_BYTES {
            return Err(AdapterError::Limit("model bytes"));
        }
    }
    Ok(())
}

fn preflight_messages(messages: &[Value]) -> Result<(), AdapterError> {
    let mut page_bytes = 0_usize;
    for message in messages {
        if !matches!(
            message.get("role").and_then(Value::as_str),
            Some("user" | "assistant" | "toolResult" | "tool")
        ) {
            continue;
        }
        let content = message.get("content").ok_or(AdapterError::Malformed)?;
        let item_bytes = match content {
            Value::String(text) => checked_text_bytes(text, 0)?,
            Value::Array(parts) => {
                let mut item_bytes = 0_usize;
                for part in parts {
                    if part.get("type").and_then(Value::as_str) == Some("text")
                        && let Some(text) = part.get("text").and_then(Value::as_str)
                    {
                        item_bytes = checked_text_bytes(text, item_bytes)?;
                    }
                }
                item_bytes
            }
            _ => return Err(AdapterError::Malformed),
        };
        page_bytes = page_bytes
            .checked_add(item_bytes)
            .ok_or(AdapterError::Limit("message page bytes"))?;
        if page_bytes > PAGE_BYTES {
            return Err(AdapterError::Limit("message page bytes"));
        }
    }
    Ok(())
}

fn preflight_entries(entries: &[Value]) -> Result<(), AdapterError> {
    let mut total = 0_usize;
    for entry in entries {
        let id = required_opaque(entry.get("id"))?;
        let kind = required_text(entry.get("type"))?;
        if kind.len() > 128 {
            return Err(AdapterError::Limit("text bytes"));
        }
        total = total
            .checked_add(id.len())
            .and_then(|value| value.checked_add(kind.len()))
            .ok_or(AdapterError::Limit("entry page bytes"))?;
        if total > PAGE_BYTES {
            return Err(AdapterError::Limit("entry page bytes"));
        }
    }
    Ok(())
}

fn checked_text_bytes(text: &str, current: usize) -> Result<usize, AdapterError> {
    if text.contains('\0') {
        return Err(AdapterError::Limit("message bytes"));
    }
    let total = current
        .checked_add(text.len())
        .ok_or(AdapterError::Limit("message bytes"))?;
    if total > ITEM_BYTES {
        return Err(AdapterError::Limit("message bytes"));
    }
    Ok(total)
}

fn optional_opaque(value: Option<&Value>) -> Result<&str, AdapterError> {
    let Some(value) = value else { return Ok("") };
    let text = required_text(Some(value))?;
    validate_opaque_text(text, EXTERNAL_ID_BYTES)?;
    Ok(text)
}

fn required_opaque(value: Option<&Value>) -> Result<&str, AdapterError> {
    let text = required_text(value)?;
    validate_opaque_text(text, EXTERNAL_ID_BYTES)?;
    Ok(text)
}

fn required_text(value: Option<&Value>) -> Result<&str, AdapterError> {
    let text = value
        .and_then(Value::as_str)
        .ok_or(AdapterError::Malformed)?;
    if text.contains('\0') {
        return Err(AdapterError::Limit("text bytes"));
    }
    Ok(text)
}
