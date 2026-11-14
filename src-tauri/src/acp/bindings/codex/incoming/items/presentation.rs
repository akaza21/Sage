use serde_json::{Map, Value};

use sage_acp_protocol::{
    contract::{BoundedVec, SafeText},
    declared_test::CommandActivity,
    intent::{NormalizedPayload, SafeResourceLocation, ToolCategory, ToolExecution},
    lifecycle::{ItemLifecycle, ToolState},
};
use sage_core::limits::COMMAND_RESOURCE_LIMIT;

use super::super::{IngressError, support::content};
use super::{ItemNormalization, sanitize};

const COMMAND_LABEL_CHARS: usize = 200;
const COMMAND_LABEL_BYTES: usize = 500;
const ITEM_SUMMARY_BYTES: usize = 65_536;

pub(super) type ToolLocations = BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>;

pub(super) struct ToolPresentationInput<'a> {
    pub category: ToolCategory,
    pub label: &'a str,
    pub tool_state: ToolState,
    pub input_summary: Option<String>,
    pub output_summary: Option<String>,
    pub locations: ToolLocations,
    pub command_activity: CommandActivity,
    pub execution: ToolExecution,
}

pub(super) fn bounded_label(value: Option<&str>, fallback: &str) -> String {
    let value = value.filter(|value| !value.is_empty()).unwrap_or(fallback);
    let limit = value
        .char_indices()
        .take(COMMAND_LABEL_CHARS)
        .map(|(index, character)| index + character.len_utf8())
        .take_while(|end| *end <= COMMAND_LABEL_BYTES)
        .last()
        .unwrap_or_default();
    if limit == value.len() {
        return value.to_owned();
    }
    let mut label = value[..limit].to_owned();
    label.push('…');
    label
}

pub(super) fn context_compaction(
    _item: &Map<String, Value>,
    _lifecycle: ItemLifecycle,
) -> Result<ItemNormalization, IngressError> {
    Ok((
        NormalizedPayload::CompactionNotice,
        None,
        Some(ItemLifecycle::Completed),
    ))
}

pub(super) fn bounded_summary(value: &str) -> Option<String> {
    (value.len() <= ITEM_SUMMARY_BYTES)
        .then(|| sanitize(value))
        .filter(|value| !value.is_empty() && value.len() <= ITEM_SUMMARY_BYTES)
}

/// Command lines are already sanitized; clip only their display summary.
pub(super) fn bounded_command_summary(value: &str) -> String {
    let mut end = value.len().min(ITEM_SUMMARY_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

pub(super) fn parse_tool_state(status: &str) -> Option<ToolState> {
    match status {
        "inProgress" => Some(ToolState::InProgress),
        "completed" => Some(ToolState::Completed),
        "failed" => Some(ToolState::Failed),
        "declined" => Some(ToolState::Declined),
        _ => None,
    }
}

pub(super) fn declared_tool_state(item: &Map<String, Value>) -> Result<ToolState, IngressError> {
    parse_tool_state(
        item.get("status")
            .and_then(Value::as_str)
            .unwrap_or("inProgress"),
    )
    .ok_or(IngressError::Malformed)
}

pub(super) fn lenient_tool_state(item: &Map<String, Value>, lifecycle: ItemLifecycle) -> ToolState {
    item.get("status")
        .and_then(Value::as_str)
        .and_then(parse_tool_state)
        .unwrap_or(if lifecycle.is_terminal() {
            ToolState::Completed
        } else {
            ToolState::InProgress
        })
}

pub(super) fn tool(
    item: &Map<String, Value>,
    category: ToolCategory,
    label: &str,
) -> Result<ItemNormalization, IngressError> {
    tool_with_summary(item, category, label, None, None, Default::default())
}

pub(super) fn tool_with_summary(
    item: &Map<String, Value>,
    category: ToolCategory,
    label: &str,
    input_summary: Option<String>,
    output_summary: Option<String>,
    locations: ToolLocations,
) -> Result<ItemNormalization, IngressError> {
    tool_with_summary_status(ToolPresentationInput {
        category,
        label,
        tool_state: declared_tool_state(item)?,
        input_summary,
        output_summary,
        locations,
        command_activity: CommandActivity::Ordinary,
        execution: ToolExecution::default(),
    })
}

/// The item lifecycle a settled tool state ends in; `None` while it runs.
pub(super) fn terminal_lifecycle(tool_state: ToolState) -> Option<ItemLifecycle> {
    match tool_state {
        ToolState::Completed => Some(ItemLifecycle::Completed),
        ToolState::Failed => Some(ItemLifecycle::Failed),
        ToolState::Declined => Some(ItemLifecycle::Cancelled),
        _ => None,
    }
}

pub(super) fn tool_with_summary_status(
    input: ToolPresentationInput<'_>,
) -> Result<ItemNormalization, IngressError> {
    let ToolPresentationInput {
        category,
        label,
        tool_state,
        input_summary,
        output_summary,
        locations,
        command_activity,
        execution,
    } = input;
    let terminal = terminal_lifecycle(tool_state);
    let label = SafeText::new("toolLabel", label).map_err(content)?;
    let input_summary = input_summary
        .map(|value| SafeText::new("toolInput", value))
        .transpose()
        .map_err(content)?;
    let output_summary = output_summary
        .map(|value| SafeText::new("toolOutput", value))
        .transpose()
        .map_err(content)?;
    let payload = NormalizedPayload::ToolCall {
        execution,
        category,
        label,
        input_summary,
        output_summary,
        locations,
        command_activity,
    };
    Ok((payload, Some(tool_state), terminal))
}

pub(super) fn collaboration(item: &Map<String, Value>) -> Result<ItemNormalization, IngressError> {
    let is_call = item.get("type").and_then(Value::as_str) == Some("collabAgentToolCall");
    let operation = if is_call {
        item.get("tool").and_then(Value::as_str)
    } else {
        item.get("kind").and_then(Value::as_str)
    }
    .filter(|value| {
        matches!(
            *value,
            "spawnAgent"
                | "sendInput"
                | "resumeAgent"
                | "wait"
                | "closeAgent"
                | "sendMessage"
                | "followupTask"
                | "interruptAgent"
                | "listAgents"
                | "started"
                | "interacted"
                | "interrupted"
                | "completed"
        )
    });
    let agent_key = if is_call {
        None
    } else {
        item.get("agentThreadId").and_then(Value::as_str)
    }
    .filter(|value| !value.is_empty() && value.len() <= 512)
    .map(|value| SafeText::new("agentKey", value))
    .transpose()
    .map_err(content)?;
    let receiver_ids = item
        .get("receiverThreadIds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(32)
        .filter_map(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 512)
        .map(|value| SafeText::new("agentKey", value))
        .collect::<Result<Vec<_>, _>>()
        .map_err(content)?;
    let states = item.get("agentsStates").and_then(Value::as_object);
    let agent_states = receiver_ids
        .iter()
        .filter_map(|id| {
            let status = states?.get(id.as_str())?.get("status")?.as_str()?;
            matches!(
                status,
                "pendingInit"
                    | "running"
                    | "interrupted"
                    | "completed"
                    | "errored"
                    | "shutdown"
                    | "notFound"
            )
            .then(|| {
                SafeText::new("agentState", status)
                    .ok()
                    .map(|state| (id.clone(), state))
            })
            .flatten()
        })
        .collect();
    let display = item
        .get("agentPath")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 512
                && !value.chars().any(|character| {
                    character.is_control() || matches!(character, '/' | '\\' | ':')
                })
        })
        .unwrap_or("Subagent");
    let label = if is_call {
        operation.unwrap_or("Subagent")
    } else {
        display
    };
    Ok((
        NormalizedPayload::Collaboration {
            label: SafeText::new("activityLabel", label).map_err(content)?,
            message: None,
            receiver_labels: BoundedVec::default(),
            operation: operation
                .map(|value| SafeText::new("operation", value))
                .transpose()
                .map_err(content)?,
            agent_key,
            agent_keys: BoundedVec::new("agentKeys", receiver_ids).map_err(content)?,
            agent_states: BoundedVec::new("agentStates", agent_states).map_err(content)?,
            model: spawn_text(item, is_call, "model"),
            reasoning_effort: spawn_text(item, is_call, "reasoningEffort"),
        },
        None,
        None,
    ))
}

fn spawn_text<const N: usize>(
    item: &Map<String, Value>,
    is_call: bool,
    field: &'static str,
) -> Option<SafeText<N>> {
    item.get(field)
        .and_then(Value::as_str)
        .filter(|value| {
            is_call && !value.is_empty() && value.len() <= N && !value.chars().any(char::is_control)
        })
        .and_then(|value| SafeText::new(field, value).ok())
}
