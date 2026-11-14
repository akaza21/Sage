use crate::{
    acp::{
        contract::{BoundedVec, SafeText},
        intent::NormalizedPayload,
        lifecycle::ItemLifecycle,
    },
    acp_protocol::codex_protocol::ChildTextRole,
};

use super::*;

/// Subagents one session follows at once; more show as ordinary tool rows.
pub(crate) const OPEN_CHILDREN: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ChildEvent {
    Spawned {
        key: String,
        label: String,
        prompt: String,
        model: Option<String>,
    },
    Update {
        key: String,
        update: Value,
    },
    Ended {
        key: String,
        outcome: ChildOutcome,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildOutcome {
    Completed,
    Failed,
    Stopped,
}

#[derive(Default)]
pub(crate) struct Children(BTreeMap<String, (ClaudeTranslator, Row)>);

#[derive(Clone)]
struct Row {
    label: String,
    prompt: String,
    model: Option<String>,
}

impl ClaudeRuntime {
    pub(crate) fn child_event(
        &self,
        children: &mut Children,
        event: ChildEvent,
        resources: &dyn SharedResourceMapper,
    ) -> Result<Vec<SharedEvent>, AdapterRuntimeError> {
        let Some(active) = self.active.as_ref().filter(|active| active.turn.is_some()) else {
            children.0.clear();
            return Ok(Vec::new());
        };
        let mut events = Vec::new();
        match event {
            ChildEvent::Spawned {
                key,
                label,
                prompt,
                model,
            } => {
                if children.0.len() >= OPEN_CHILDREN || children.0.contains_key(&key) {
                    return Ok(events);
                }
                let mut translator = ClaudeTranslator::new(
                    super::super::claude_profile(),
                    active.external_id.as_str(),
                )
                .map_err(map_translation)?;
                translator.begin_prompt(&key).map_err(map_translation)?;
                let row = Row {
                    label,
                    prompt,
                    model,
                };
                events.push(self.child_row(active, &key, &row, "running")?);
                if !row.prompt.is_empty() {
                    let text = SafeText::new("text", row.prompt.as_str()).map_err(frame)?;
                    events.push(live(
                        &key,
                        "prompt",
                        Some(ChildTextRole::User),
                        ItemLifecycle::Completed,
                        None,
                        NormalizedPayload::Text { text },
                    )?);
                }
                children.0.insert(key, (translator, row));
            }
            ChildEvent::Update { key, update } => {
                let Some((translator, _)) = children.0.get_mut(&key) else {
                    return Ok(events);
                };
                // A subagent frame Sage cannot show leaves the parent turn running.
                let Ok(translated) =
                    translator.translate_notification_value(update, &MapperBridge(resources))
                else {
                    return Ok(events);
                };
                for event in translated {
                    if let AdapterEvent::Item {
                        external_key,
                        lifecycle,
                        tool_state,
                        payload,
                    } = event
                    {
                        let role = matches!(payload, NormalizedPayload::Text { .. })
                            .then_some(ChildTextRole::Assistant);
                        let item = external_key.as_str();
                        events.push(live(&key, item, role, lifecycle, tool_state, payload)?);
                    }
                }
            }
            ChildEvent::Ended { key, outcome } => {
                let Some((_, row)) = children.0.remove(&key) else {
                    return Ok(events);
                };
                let (state, status) = match outcome {
                    ChildOutcome::Completed => ("completed", TurnState::Completed),
                    ChildOutcome::Failed => ("errored", TurnState::Failed),
                    ChildOutcome::Stopped => ("interrupted", TurnState::Interrupted),
                };
                events.push(self.child_row(active, &key, &row, state)?);
                let id = ExternalId::new("childThreadId", key).map_err(frame)?;
                events.push(SharedEvent::ChildTurnStatus {
                    child_thread_id: id.clone(),
                    child_turn_id: id,
                    status,
                });
            }
        }
        Ok(events)
    }

    fn child_row(
        &self,
        active: &ActiveSession,
        key: &str,
        row: &Row,
        state: &str,
    ) -> Result<SharedEvent, AdapterRuntimeError> {
        let agent = SafeText::new("agentKey", key).map_err(frame)?;
        let label = if row.label.is_empty() {
            "Subagent"
        } else {
            &row.label
        };
        let external = ExternalId::new("externalItemId", format!("agent:{key}")).map_err(frame)?;
        Ok(SharedEvent::ChildThreadItem {
            id: self.item_identity(active, external)?,
            lifecycle: if state == "running" {
                ItemLifecycle::Started
            } else {
                ItemLifecycle::Completed
            },
            payload: NormalizedPayload::Collaboration {
                label: SafeText::new("label", "Subagent").map_err(frame)?,
                message: (!row.prompt.is_empty())
                    .then(|| SafeText::new("message", row.prompt.as_str()))
                    .transpose()
                    .map_err(frame)?,
                receiver_labels: BoundedVec::new(
                    "receiverLabels",
                    vec![SafeText::new("receiverLabel", label).map_err(frame)?],
                )
                .map_err(frame)?,
                operation: Some(SafeText::new("operation", "spawnAgent").map_err(frame)?),
                agent_key: None,
                agent_keys: BoundedVec::new("agentKeys", vec![agent.clone()]).map_err(frame)?,
                agent_states: BoundedVec::new(
                    "agentStates",
                    vec![(agent, SafeText::new("agentState", state).map_err(frame)?)],
                )
                .map_err(frame)?,
                model: row
                    .model
                    .as_deref()
                    .map(|model| SafeText::new("model", model))
                    .transpose()
                    .map_err(frame)?,
                reasoning_effort: None,
            },
            child_thread_id: ExternalId::new("childThreadId", key).map_err(frame)?,
        })
    }
}

fn frame(_: impl std::fmt::Debug) -> AdapterRuntimeError {
    AdapterRuntimeError::Frame
}

fn live(
    key: &str,
    item: &str,
    role: Option<ChildTextRole>,
    lifecycle: ItemLifecycle,
    tool_state: Option<crate::acp::lifecycle::ToolState>,
    payload: NormalizedPayload,
) -> Result<SharedEvent, AdapterRuntimeError> {
    let child = ExternalId::new("childThreadId", key).map_err(frame)?;
    Ok(SharedEvent::ChildLiveItem {
        child_thread_id: child.clone(),
        child_turn_id: child,
        item_id: ExternalId::new("childItemId", item).map_err(frame)?,
        role,
        lifecycle,
        tool_state,
        payload,
    })
}
