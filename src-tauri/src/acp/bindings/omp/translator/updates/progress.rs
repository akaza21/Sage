use super::*;

pub(super) fn plan_event(update: &Map<String, Value>) -> Result<AdapterEvent, TranslationError> {
    let entries = update
        .get("entries")
        .and_then(Value::as_array)
        .ok_or(TranslationError::MalformedFrame)?;
    if entries.len() > MAX_PLAN_ENTRIES {
        return Err(TranslationError::Limit("plan entries"));
    }
    let mut summary = String::new();
    for (index, entry) in entries.iter().enumerate() {
        if index != 0 {
            summary.push('\n');
        }
        let status = normalized_text::<32>(
            "planStatus",
            entry
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending"),
        )?;
        let content = normalized_text::<262144>(
            "planEntry",
            entry
                .get("content")
                .and_then(Value::as_str)
                .ok_or(TranslationError::MalformedFrame)?,
        )?;
        summary.push('[');
        summary.push_str(status.as_str());
        summary.push_str("] ");
        summary.push_str(content.as_str());
        if summary.len() > 262_144 {
            return Err(TranslationError::Limit("plan bytes"));
        }
    }
    Ok(AdapterEvent::Item {
        external_key: optional_item_id(update, "plan")?,
        lifecycle: ItemLifecycle::Streaming,
        tool_state: None,
        payload: NormalizedPayload::Plan {
            summary: SafeText::new("plan", summary)?,
        },
        update_mode: ItemUpdateMode::Replace,
    })
}

pub(super) fn usage_event(update: &Map<String, Value>) -> Result<AdapterEvent, TranslationError> {
    let usage = update
        .get("usage")
        .and_then(Value::as_object)
        .unwrap_or(update);
    Ok(AdapterEvent::Item {
        external_key: optional_item_id(update, "usage")?,
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        payload: NormalizedPayload::Usage {
            input_tokens: usage
                .get("inputTokens")
                .or_else(|| usage.get("input_tokens"))
                .and_then(Value::as_u64)
                .ok_or(TranslationError::MalformedFrame)?,
            context_window: None,
            output_tokens: usage
                .get("outputTokens")
                .or_else(|| usage.get("output_tokens"))
                .and_then(Value::as_u64)
                .ok_or(TranslationError::MalformedFrame)?,
        },
        update_mode: ItemUpdateMode::Replace,
    })
}
