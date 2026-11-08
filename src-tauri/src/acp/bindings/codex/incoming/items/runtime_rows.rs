use serde_json::{Map, Value};

use crate::acp::{
    contract::BoundedVec,
    declared_test::CommandActivity,
    intent::{NormalizedPayload, ToolCategory, ToolExecution},
    lifecycle::ItemLifecycle,
    limits::LIMITS,
};

use super::super::{IngressError, support::safe_text};
use super::presentation::{
    ToolPresentationInput, bounded_label, bounded_summary, lenient_tool_state,
    tool_with_summary_status,
};
use super::{ItemNormalization, sanitize};

const JOINED_PARTS_MAX: usize = 32;

fn row(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
    category: ToolCategory,
    label: &str,
    input_summary: Option<String>,
    output_summary: Option<String>,
) -> Result<ItemNormalization, IngressError> {
    tool_with_summary_status(ToolPresentationInput {
        category,
        label,
        tool_state: lenient_tool_state(item, lifecycle),
        input_summary,
        output_summary,
        locations: BoundedVec::default(),
        command_activity: CommandActivity::Ordinary,
        execution: ToolExecution::default(),
    })
}

fn review_text(item: &Map<String, Value>) -> Option<String> {
    item.get("review")
        .and_then(Value::as_str)
        .and_then(bounded_summary)
}

pub(super) fn entered_review(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
) -> Result<ItemNormalization, IngressError> {
    row(
        item,
        lifecycle,
        ToolCategory::SwitchMode,
        "Entered review mode",
        review_text(item),
        None,
    )
}

/// The review's findings are the review Turn's reply.
pub(super) fn exited_review(
    item: &Map<String, Value>,
) -> Result<Option<ItemNormalization>, IngressError> {
    if item
        .get("review")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Ok(None);
    }
    let text = safe_text(
        item.get("review"),
        LIMITS.wire.normalized_item_bytes as usize,
        "review",
    )?;
    Ok(Some((NormalizedPayload::Text { text }, None, None)))
}

pub(super) fn sleep(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
) -> Result<ItemNormalization, IngressError> {
    let duration = item
        .get("durationMs")
        .and_then(Value::as_u64)
        .ok_or(IngressError::Malformed)?;
    let verb = if lifecycle.is_terminal() {
        "Waited"
    } else {
        "Waiting"
    };
    row(
        item,
        lifecycle,
        ToolCategory::Other,
        &format!("{verb} {}", duration_label(duration)),
        None,
        None,
    )
}

fn duration_label(millis: u64) -> String {
    match millis {
        0..1_000 => format!("{millis} ms"),
        1_000..60_000 if millis.is_multiple_of(1_000) => format!("{}s", millis / 1_000),
        1_000..60_000 => format!("{:.1}s", millis as f64 / 1_000.0),
        _ => match (millis / 60_000, millis % 60_000 / 1_000) {
            (minutes, 0) => format!("{minutes}m"),
            (minutes, seconds) => format!("{minutes}m {seconds}s"),
        },
    }
}

/// A tool's returned output, as text; media and encrypted parts are named,
/// never inlined.
pub(super) fn function_call_output(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
) -> Result<ItemNormalization, IngressError> {
    let name = item.get("name").and_then(Value::as_str).map(sanitize);
    let output = match item.get("output") {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Array(parts)) => Some(
            parts
                .iter()
                .take(JOINED_PARTS_MAX)
                .filter_map(|part| match part.get("type").and_then(Value::as_str)? {
                    "input_text" => part.get("text").and_then(Value::as_str),
                    "input_image" => Some("[image]"),
                    "input_audio" => Some("[audio]"),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => None,
    };
    row(
        item,
        lifecycle,
        ToolCategory::Other,
        &bounded_label(name.as_deref(), "Tool output"),
        None,
        output.as_deref().and_then(bounded_summary),
    )
}

pub(super) fn hook_prompt(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
) -> Result<ItemNormalization, IngressError> {
    let text = item
        .get("fragments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(JOINED_PARTS_MAX)
        .filter_map(|fragment| fragment.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n\n");
    row(
        item,
        lifecycle,
        ToolCategory::Other,
        "Hook prompt",
        bounded_summary(&text),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_people_say_them() {
        for (millis, label) in [
            (250, "250 ms"),
            (2_000, "2s"),
            (1_500, "1.5s"),
            (60_000, "1m"),
            (95_000, "1m 35s"),
        ] {
            assert_eq!(duration_label(millis), label);
        }
    }
}
