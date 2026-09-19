use crate::acp::{
    contract::{
        BoundedVec, ContractError, EntityRevision, RuntimeEpoch, SafeText, SageActivityId,
        SageAgentSessionId, SageChatId, SageItemId, SageProjectId, SageRootId, SageServerRequestId,
        SageTurnId,
    },
    intent::NormalizedPayload,
    lifecycle::ServerRequestState,
    projection::{
        FileChangeProjection, FileOperation, ItemContent, ItemOrigin, ItemProjection, PlanEntry,
        PlanEntryState, TextRole,
    },
};

use super::StoreError;

mod resource_locations;

pub(super) struct ItemRow {
    pub id: String,
    pub project_id: String,
    pub chat_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub runtime_epoch: i64,
    pub ordinal: i64,
    pub origin: String,
    pub activity_id: Option<String>,
    pub kind: String,
    pub lifecycle: String,
    pub tool_state: Option<String>,
    pub external_item_id: Option<String>,
    pub payload_json: String,
    pub payload_bytes: i64,
    pub payload_digest: String,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub terminal_at: Option<i64>,
}

pub(super) fn decode_item(
    row: ItemRow,
    permission: Option<(SageServerRequestId, ServerRequestState)>,
    project_roots: &[SageRootId],
) -> Result<ItemProjection, StoreError> {
    if row.payload_bytes < 0
        || row.payload_bytes as usize != row.payload_json.len()
        || super::facts::hex_digest(row.payload_json.as_bytes()) != row.payload_digest
    {
        return Err(StoreError::Corrupt("item payload integrity"));
    }
    let (normalized, legacy_role) =
        match serde_json::from_str::<NormalizedPayload>(&row.payload_json) {
            Ok(value) => (value, None),
            Err(_) => parse_legacy_text(&row.payload_json)
                .map_err(|_| StoreError::Corrupt("normalized item payload"))?,
        };
    if kind_text(normalized.kind()) != row.kind {
        return Err(StoreError::Corrupt("item kind mismatch"));
    }
    resource_locations::validate_membership(&normalized, project_roots)?;
    if matches!(normalized, NormalizedPayload::PermissionRequest { .. })
        && row.external_item_id.is_none()
    {
        return Err(StoreError::Corrupt("permission item external id"));
    }
    let activity_id = row
        .activity_id
        .map(SageActivityId::parse)
        .transpose()
        .map_err(|_| StoreError::Corrupt("activity id"))?;
    let lifecycle = super::state::parse_item_lifecycle(&row.lifecycle)?;
    let tool_state = row
        .tool_state
        .as_deref()
        .map(super::state::parse_tool_state)
        .transpose()?;
    let content = content(
        normalized,
        lifecycle,
        legacy_role,
        activity_id.as_ref(),
        permission,
    )?;
    if content.requires_tool_state() != tool_state.is_some() {
        return Err(StoreError::Corrupt("item tool state"));
    }
    Ok(ItemProjection {
        id: parse(row.id, SageItemId::parse, "item id")?,
        project_id: parse(row.project_id, SageProjectId::parse, "project id")?,
        chat_id: parse(row.chat_id, SageChatId::parse, "chat id")?,
        session_id: parse(row.session_id, SageAgentSessionId::parse, "session id")?,
        turn_id: parse(row.turn_id, SageTurnId::parse, "turn id")?,
        runtime_epoch: positive(row.runtime_epoch, RuntimeEpoch::new, "runtime epoch")?,
        ordinal: nonnegative(row.ordinal, "item ordinal")?,
        origin: match row.origin.as_str() {
            "primary" => ItemOrigin::Primary,
            "harnessActivity" => ItemOrigin::HarnessActivity,
            _ => return Err(StoreError::Corrupt("item origin")),
        },
        activity_id,
        lifecycle,
        revision: revision(row.revision)?,
        created_at: row.created_at,
        updated_at: row.updated_at,
        terminal_at: row.terminal_at,
        tool_state,
        content,
    })
}

fn content(
    value: NormalizedPayload,
    lifecycle: crate::acp::lifecycle::ItemLifecycle,
    legacy_role: Option<TextRole>,
    activity: Option<&SageActivityId>,
    permission: Option<(SageServerRequestId, ServerRequestState)>,
) -> Result<ItemContent, StoreError> {
    Ok(match value {
        NormalizedPayload::Text { text } => ItemContent::Text {
            role: legacy_role.unwrap_or(TextRole::Assistant),
            text,
        },
        NormalizedPayload::ThoughtSummary { summary } => ItemContent::ThoughtSummary { summary },
        NormalizedPayload::Plan { summary } => ItemContent::Plan {
            entries: BoundedVec::new(
                "planEntries",
                vec![PlanEntry {
                    label: summary_label(summary.as_str())?,
                    state: if lifecycle.is_terminal() {
                        PlanEntryState::Completed
                    } else {
                        PlanEntryState::InProgress
                    },
                }],
            )
            .map_err(contract)?,
        },
        NormalizedPayload::ToolCall {
            category,
            label,
            input_summary,
            output_summary,
            locations,
            command_activity,
        } => ItemContent::ToolCall {
            presentation: presentation(category, label, locations, command_activity)?,
            input_summary,
            output_summary,
        },
        NormalizedPayload::ToolUpdateResult {
            category,
            label,
            output_summary,
            locations,
            command_activity,
        } => ItemContent::ToolUpdateResult {
            call_item_id: None,
            presentation: presentation(category, label, locations, command_activity)?,
            output_summary,
        },
        NormalizedPayload::FileChangeDiff {
            resource,
            summary,
            additions,
            deletions,
            truncated,
            snapshot_revision,
            old_bytes,
            new_bytes,
            content_digest,
            pair_available,
        } => {
            let pair = admitted_pair_view(
                truncated,
                snapshot_revision,
                old_bytes,
                new_bytes,
                content_digest,
                pair_available,
            )?;
            ItemContent::FileChangeDiff {
                changes: BoundedVec::new(
                    "fileChanges",
                    vec![FileChangeProjection {
                        resource,
                        operation: FileOperation::Updated,
                        original_resource: None,
                        summary,
                        additions: projected_line_count(additions),
                        deletions: projected_line_count(deletions),
                        pair_state: pair.state,
                        snapshot_revision: pair.revision,
                        old_bytes: pair.old_bytes,
                        new_bytes: pair.new_bytes,
                        content_digest: pair.digest,
                    }],
                )
                .map_err(contract)?,
                truncated,
            }
        }
        NormalizedPayload::Image { media_type, data } => ItemContent::Image {
            media_type,
            data_url: crate::acp::image_admit::image_data_url(media_type, data.as_str())
                .map_err(contract)?,
        },
        NormalizedPayload::Usage {
            input_tokens,
            output_tokens,
            context_window,
        } => ItemContent::Usage {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            cached_input_tokens: None,
            total_tokens: input_tokens.checked_add(output_tokens),
            context_window,
        },
        NormalizedPayload::PermissionRequest { summary, .. } => {
            let (request_id, state) =
                permission.ok_or(StoreError::Corrupt("permission request projection"))?;
            ItemContent::PermissionRequest {
                request_id,
                state,
                label: SafeText::new("permissionLabel", "Permission request").map_err(contract)?,
                summary,
            }
        }
        NormalizedPayload::Error {
            code,
            message,
            retryable,
        } => ItemContent::Error {
            code,
            message,
            recoverable: retryable,
        },
        NormalizedPayload::Unsupported {
            external_kind,
            summary,
        } => ItemContent::Unsupported {
            category: external_kind,
            summary,
            truncated: false,
        },
        NormalizedPayload::Collaboration {
            label,
            message,
            receiver_labels,
        } => ItemContent::Collaboration {
            activity_id: activity
                .cloned()
                .ok_or(StoreError::Corrupt("collaboration activity"))?,
            label,
            summary: message.unwrap_or(SafeText::new("summary", "").map_err(contract)?),
            receiver_labels,
        },
    })
}

fn projected_line_count(value: Option<u64>) -> Option<u32> {
    value
        .filter(|count| *count <= 1_000_000)
        .and_then(|count| u32::try_from(count).ok())
}

/// Legacy summary-only rows (no pair metadata) decode as unavailable, never
/// as fabricated empty text. Malformed admitted metadata fails closed.
mod file_pair;
mod tool_presentation;
use file_pair::admitted_pair_view;
use tool_presentation::presentation;

fn parse_legacy_text(
    value: &str,
) -> Result<(NormalizedPayload, Option<TextRole>), serde_json::Error> {
    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        role: String,
        text: SafeText<262144>,
    }
    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TaggedWire {
        kind: String,
        payload: Wire,
    }
    let wire = match serde_json::from_str::<Wire>(value) {
        Ok(wire) => wire,
        Err(_) => {
            let tagged: TaggedWire = serde_json::from_str(value)?;
            if tagged.kind != "text" {
                return Err(serde_json::from_str::<Wire>("null").unwrap_err());
            }
            tagged.payload
        }
    };
    let role = match wire.role.as_str() {
        "user" => TextRole::User,
        "assistant" => TextRole::Assistant,
        "system" => TextRole::System,
        _ => return Err(serde_json::from_str::<Wire>("null").unwrap_err()),
    };
    Ok((NormalizedPayload::Text { text: wire.text }, Some(role)))
}

fn summary_label(value: &str) -> Result<SafeText<512>, StoreError> {
    let mut end = value.len().min(512);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    SafeText::new("planLabel", &value[..end]).map_err(contract)
}
fn kind_text(value: crate::acp::contract::ItemKind) -> &'static str {
    value.as_str()
}
fn revision(value: i64) -> Result<EntityRevision, StoreError> {
    EntityRevision::new(value as u64).map_err(|_| StoreError::Corrupt("entity revision"))
}
fn nonnegative(value: i64, label: &'static str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt(label))
}
fn positive<T, E>(
    value: i64,
    constructor: fn(u64) -> Result<T, E>,
    label: &'static str,
) -> Result<T, StoreError> {
    constructor(u64::try_from(value).map_err(|_| StoreError::Corrupt(label))?)
        .map_err(|_| StoreError::Corrupt(label))
}
fn parse<T, E>(
    value: String,
    constructor: fn(String) -> Result<T, E>,
    label: &'static str,
) -> Result<T, StoreError> {
    constructor(value).map_err(|_| StoreError::Corrupt(label))
}
fn contract(_: ContractError) -> StoreError {
    StoreError::Corrupt("projection contract")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_projection_tracks_streaming_and_terminal_lifecycle() {
        let payload = NormalizedPayload::Plan {
            summary: SafeText::new("plan", "Inspect the project").unwrap(),
        };
        let streaming = content(
            payload.clone(),
            crate::acp::lifecycle::ItemLifecycle::Streaming,
            None,
            None,
            None,
        )
        .unwrap();
        let completed = content(
            payload,
            crate::acp::lifecycle::ItemLifecycle::Completed,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(matches!(
            streaming,
            ItemContent::Plan { ref entries }
                if entries.as_slice()[0].state == PlanEntryState::InProgress
        ));
        assert!(matches!(
            completed,
            ItemContent::Plan { ref entries }
                if entries.as_slice()[0].state == PlanEntryState::Completed
        ));
    }
}
