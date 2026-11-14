use serde_json::{Map, Value};

use crate::acp::{
    contract::{BoundedVec, RequiredText, SafeText},
    intent::{
        ExternalId, NormalizedPayload, NormalizedPlanEntry, NormalizedPlanState,
        PermissionOptionKind, ToolCategory,
    },
    lifecycle::{ItemLifecycle, ToolState},
};
use sage_core::limits::LIMITS;

use super::super::profile::StandardAcpProfile;
use super::super::sanitize::normalized_text;
use super::{AdapterEvent, TranslationError};

const MAX_PLAN_ENTRIES: usize = 128;
const PERMISSION_ENVELOPE_BYTES: usize = 512;

pub(super) fn structural_subagent(update: &Map<String, Value>) -> bool {
    let Some(meta) = update.get("_meta").and_then(Value::as_object) else {
        return false;
    };
    [
        meta.get("toolName"),
        meta.get("claudeCode").and_then(|v| v.get("toolName")),
        meta.get("opencode").and_then(|v| v.get("toolName")),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .any(|name| matches!(name, "Task" | "Agent"))
}

pub(super) fn fits_permission_payload(summary: &str) -> bool {
    escaped_bytes(summary).is_some_and(|escaped| {
        escaped.saturating_add(PERMISSION_ENVELOPE_BYTES)
            <= LIMITS.wire.permission_payload_bytes as usize
    })
}

fn escaped_bytes(value: &str) -> Option<usize> {
    value.chars().try_fold(0usize, |total, character| {
        let width = match character {
            '"' | '\\' | '\u{8}' | '\u{9}' | '\u{a}' | '\u{c}' | '\u{d}' => 2,
            '\u{0}'..='\u{1f}' => 6,
            other => other.len_utf8(),
        };
        total.checked_add(width)
    })
}

pub(super) fn offered_option_kinds(
    params: &Map<String, Value>,
) -> Result<Option<BoundedVec<PermissionOptionKind, 8>>, TranslationError> {
    params
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            if options.len() > 8 {
                return Err(TranslationError::PermissionLimit);
            }
            let kinds = options
                .iter()
                .filter(|option| {
                    option
                        .get("optionId")
                        .and_then(Value::as_str)
                        .is_some_and(|id| !id.is_empty() && id.len() <= 64)
                })
                .filter_map(|option| {
                    match option
                        .get("kind")
                        .and_then(Value::as_str)
                        .filter(|kind| !kind.is_empty())
                        .or_else(|| option.get("name").and_then(Value::as_str))
                        .filter(|text| text.len() <= 64)?
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "allow_once" => Some(PermissionOptionKind::AllowOnce),
                        "allow_always" => Some(PermissionOptionKind::AllowAlways),
                        "reject_once" => Some(PermissionOptionKind::RejectOnce),
                        "reject_always" => Some(PermissionOptionKind::RejectAlways),
                        "allow" | "allow once" => Some(PermissionOptionKind::AllowOnce),
                        "allow always" => Some(PermissionOptionKind::AllowAlways),
                        "deny" | "reject" | "reject once" => Some(PermissionOptionKind::RejectOnce),
                        "deny always" | "reject always" => Some(PermissionOptionKind::RejectAlways),
                        _ => None,
                    }
                })
                .collect();
            BoundedVec::new("permissionOptionKinds", kinds).map_err(Into::into)
        })
        .transpose()
}

pub(super) fn plan_event(
    update: &Map<String, Value>,
    external_key: ExternalId,
) -> Result<AdapterEvent, TranslationError> {
    let entries = update
        .get("entries")
        .and_then(Value::as_array)
        .ok_or(TranslationError::MalformedFrame)?;
    if entries.len() > MAX_PLAN_ENTRIES {
        return Err(TranslationError::PlanLimit);
    }
    let mut normalized = Vec::with_capacity(entries.len());
    for entry in entries {
        let content = entry
            .get("content")
            .and_then(Value::as_str)
            .ok_or(TranslationError::MalformedFrame)?;
        let state = match entry
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("pending")
        {
            "pending" => NormalizedPlanState::Pending,
            "in_progress" | "inProgress" => NormalizedPlanState::InProgress,
            "completed" => NormalizedPlanState::Completed,
            "failed" => NormalizedPlanState::Failed,
            _ => return Err(TranslationError::MalformedFrame),
        };
        normalized.push(NormalizedPlanEntry {
            label: normalized_text::<512>("planEntry", content)?,
            state,
        });
    }
    Ok(AdapterEvent::Item {
        external_key,
        lifecycle: ItemLifecycle::Streaming,
        tool_state: None,
        payload: NormalizedPayload::PlanEntries {
            entries: BoundedVec::new("planEntries", normalized)?,
        },
    })
}

#[cfg(test)]
#[test]
fn plan_event_preserves_entry_states() {
    let update = serde_json::json!({"entries": [
        {"content":"Inspect", "status":"completed"},
        {"content":"Patch", "status":"in_progress"}
    ]});
    let event = plan_event(
        update.as_object().unwrap(),
        ExternalId::new("plan", "plan-1").unwrap(),
    )
    .unwrap();
    assert!(matches!(event, AdapterEvent::Item {
        payload: NormalizedPayload::PlanEntries { entries }, ..
    } if entries.as_slice().len() == 2
        && entries.as_slice()[0].state == NormalizedPlanState::Completed
        && entries.as_slice()[1].state == NormalizedPlanState::InProgress));
}

pub(super) fn usage_object(parent: &Map<String, Value>) -> &Map<String, Value> {
    parent
        .get("usage")
        .and_then(Value::as_object)
        .unwrap_or(parent)
}

pub(super) fn stated_context_window(usage: &Map<String, Value>) -> Option<u32> {
    usage
        .get("contextWindow")
        .or_else(|| usage.get("size"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

pub(super) fn usage_event(
    usage: &Map<String, Value>,
    context_window: Option<u32>,
) -> Option<AdapterEvent> {
    let input_tokens = usage.get("inputTokens").and_then(Value::as_u64)?;
    let output_tokens = usage.get("outputTokens").and_then(Value::as_u64)?;
    Some(AdapterEvent::Usage(NormalizedPayload::Usage {
        input_tokens,
        output_tokens,
        context_window,
    }))
}

pub(super) fn unsupported_event(
    kind: &str,
    summary: &str,
    turn_scope: Option<&str>,
) -> Result<AdapterEvent, TranslationError> {
    Ok(AdapterEvent::Item {
        external_key: turn_scoped_key(&format!("unsupported:{kind}"), turn_scope)?,
        lifecycle: ItemLifecycle::Unsupported,
        tool_state: None,
        payload: NormalizedPayload::Unsupported {
            external_kind: SafeText::new("externalKind", kind)?,
            summary: SafeText::new("unsupportedSummary", summary)?,
        },
    })
}

pub(super) fn optional_external_key(
    object: &Map<String, Value>,
    fallback: &str,
    turn_scope: Option<&str>,
) -> Result<ExternalId, TranslationError> {
    match object.get("itemId").and_then(Value::as_str) {
        Some(stated) => RequiredText::new("externalItemId", stated).map_err(Into::into),
        None => turn_scoped_key(fallback, turn_scope),
    }
}

pub(super) fn turn_scoped_key(
    base: &str,
    turn_scope: Option<&str>,
) -> Result<ExternalId, TranslationError> {
    match turn_scope {
        Some(scope) => RequiredText::new("externalItemId", format!("{base}:{scope}")),
        None => RequiredText::new("externalItemId", base),
    }
    .map_err(Into::into)
}

pub(super) fn category_label(
    profile: &'static StandardAcpProfile,
    category: ToolCategory,
) -> &'static str {
    match category {
        ToolCategory::Read => "File tool",
        ToolCategory::Edit => "Diff tool",
        ToolCategory::Delete => "Delete tool",
        ToolCategory::Move => "Move tool",
        ToolCategory::Execute => "Command tool",
        ToolCategory::Search => "Search tool",
        ToolCategory::Fetch => "Web tool",
        ToolCategory::WebSearch => "Web search",
        ToolCategory::ImageView => "Image view",
        ToolCategory::Think => "Thinking tool",
        ToolCategory::SwitchMode => "Mode switch",
        ToolCategory::Mcp => "MCP tool",
        ToolCategory::Subagent => "Nested activity",
        ToolCategory::Other => profile.other_tool_label,
    }
}

pub(super) fn parse_tool_state(
    value: Option<&Value>,
    default: ToolState,
) -> Result<ToolState, TranslationError> {
    match value.and_then(Value::as_str) {
        None => Ok(default),
        Some("pending") => Ok(ToolState::Pending),
        Some("in_progress") => Ok(ToolState::InProgress),
        Some("completed") => Ok(ToolState::Completed),
        Some("failed") => Ok(ToolState::Failed),
        _ => Err(TranslationError::UnknownToolState),
    }
}

pub(super) fn transition_tool(current: ToolState, next: ToolState) -> Result<(), TranslationError> {
    if current.can_transition_to(next) {
        Ok(())
    } else if current.is_terminal() {
        Err(TranslationError::TerminalConflict)
    } else {
        Err(TranslationError::InvalidToolTransition)
    }
}

pub(super) fn lifecycle_for_tool(state: ToolState) -> ItemLifecycle {
    match state {
        ToolState::Pending => ItemLifecycle::Started,
        ToolState::InProgress | ToolState::WaitingForApproval => ItemLifecycle::Streaming,
        ToolState::Completed | ToolState::Declined => ItemLifecycle::Completed,
        ToolState::Failed => ItemLifecycle::Failed,
        ToolState::Cancelled => ItemLifecycle::Cancelled,
    }
}

pub(super) fn rpc_id(value: Option<&Value>) -> Result<ExternalId, TranslationError> {
    let value = match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => return Err(TranslationError::MalformedFrame),
    };
    RequiredText::new("requestId", value).map_err(Into::into)
}

pub(super) fn object<'a>(
    parent: &'a Value,
    key: &str,
) -> Result<&'a Map<String, Value>, TranslationError> {
    parent
        .get(key)
        .and_then(Value::as_object)
        .ok_or(TranslationError::MalformedFrame)
}

pub(super) fn object_value(value: Option<&Value>) -> Result<&Map<String, Value>, TranslationError> {
    value
        .and_then(Value::as_object)
        .ok_or(TranslationError::MalformedFrame)
}

pub(super) fn string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, TranslationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(TranslationError::MalformedFrame)
}

#[cfg(test)]
mod permission_option_tests {
    use super::*;

    #[test]
    fn named_native_options_keep_the_harness_order() {
        let params = serde_json::json!({
            "options": [
                {"optionId": "always", "name": "Allow always"},
                {"optionId": "once", "name": "Allow once"}
            ]
        });
        let kinds = offered_option_kinds(params.as_object().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            kinds.as_slice(),
            &[
                PermissionOptionKind::AllowAlways,
                PermissionOptionKind::AllowOnce
            ]
        );
    }
}
