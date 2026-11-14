use rusqlite::{Transaction, params};

use sage_acp_protocol::{
    contract::{
        EntityRevision, ProjectionRevision, RuntimeEpoch, SageActivityId, SageAgentSessionId,
        SageChatId, SageItemId, SageProjectId, SageTurnId,
    },
    intent::{ExternalId, NormalizedPayload},
    lifecycle::{ItemLifecycle, ToolState},
};
use sage_core::limits::LIMITS;

use super::{
    EntityMutation, MutationDisposition, StoreError,
    facts::{entity_revision_from_i64, hex_digest, next_projection_revision, publish_projection},
    state::{item_lifecycle_text, tool_state_text},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemIngressCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub runtime_epoch: RuntimeEpoch,
    pub external_item_id: ExternalId,
    pub activity_id: Option<SageActivityId>,
    pub payload: NormalizedPayload,
    pub payload_digest: String,
    pub lifecycle: ItemLifecycle,
    pub tool_state: Option<ToolState>,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemIngressMutation {
    pub item_id: SageItemId,
    pub entity: EntityMutation,
}

pub(crate) fn apply_item_ingress(
    connection: &mut rusqlite::Connection,
    command: ItemIngressCommand,
) -> Result<ItemIngressMutation, StoreError> {
    let transaction = connection.transaction()?;
    let result = apply_item_ingress_in(&transaction, &command)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(result)
}

pub(super) fn apply_item_ingress_in(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
) -> Result<ItemIngressMutation, StoreError> {
    apply_ingress_in(transaction, command, false)
}

/// Only the atomic snapshot path may replace terminal FileChange metadata;
/// ownership, active Turn, resource identity and unchanged lifecycle still apply.
pub(super) fn apply_file_change_ingress_in(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
) -> Result<ItemIngressMutation, StoreError> {
    if !matches!(command.payload, NormalizedPayload::FileChangeDiff { .. }) {
        return Err(StoreError::BatchShape);
    }
    apply_ingress_in(transaction, command, true)
}

fn apply_ingress_in(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
    file_revision: bool,
) -> Result<ItemIngressMutation, StoreError> {
    validate_lifecycle(command)?;
    let payload = serde_json::to_string(&command.payload)
        .map_err(|_| StoreError::Corrupt("normalized item serialization"))?;
    let bytes = i64::try_from(payload.len()).map_err(|_| StoreError::PayloadTooLarge)?;
    if bytes > LIMITS.wire.normalized_item_bytes as i64
        || hex_digest(payload.as_bytes()) != command.payload_digest
    {
        return Err(StoreError::Corrupt("normalized item digest"));
    }
    verify_turn_owner(transaction, command)?;
    if let Some(existing) = existing_item(transaction, command)? {
        return update_existing(
            transaction,
            command,
            existing,
            payload,
            bytes,
            file_revision,
        );
    }
    let ordinal = super::item_usage::ensure_capacity(transaction, command, 1, bytes)?;
    if let Some(activity) = &command.activity_id {
        let valid: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_activities WHERE id=?1 AND project_id=?2
             AND chat_id=?3 AND session_id=?4 AND turn_id=?5 AND runtime_epoch=?6)",
            params![
                activity.as_str(),
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.turn_id.as_str(),
                command.runtime_epoch.get() as i64
            ],
            |row| row.get(0),
        )?;
        if !valid {
            return Err(StoreError::Conflict {
                resource: "activity",
            });
        }
    }
    let projection = next_projection_revision(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    let item_id = SageItemId::new();
    let terminal = command
        .lifecycle
        .is_terminal()
        .then_some(command.now_millis);
    transaction.execute(
        "INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,
         origin,activity_id,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,
         payload_digest,revision,created_at,updated_at,terminal_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,0,?17,?17,?18)",
        params![
            item_id.as_str(),
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.turn_id.as_str(),
            command.runtime_epoch.get() as i64,
            ordinal,
            if command.activity_id.is_some() {
                "harnessActivity"
            } else {
                "primary"
            },
            command.activity_id.as_ref().map(|value| value.as_str()),
            kind_text(command.payload.kind()),
            item_lifecycle_text(command.lifecycle),
            command.tool_state.map(tool_state_text),
            command.external_item_id.as_str(),
            payload,
            bytes,
            command.payload_digest,
            command.now_millis,
            terminal
        ],
    )?;
    super::item_usage::update(transaction, command, 1, bytes)?;
    super::item_usage::append_fact(transaction, command, &item_id, projection)?;
    publish_projection(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        projection,
        command.now_millis,
    )?;
    Ok(ItemIngressMutation {
        item_id,
        entity: EntityMutation {
            revision: EntityRevision::ZERO,
            projection_revision: projection,
            disposition: MutationDisposition::Applied,
        },
    })
}

struct ExistingItem {
    item_id: String,
    project_id: String,
    chat_id: String,
    turn_id: String,
    lifecycle: String,
    tool_state: Option<String>,
    activity_id: Option<String>,
    payload_digest: String,
    payload_bytes: i64,
    revision: i64,
}
fn existing_item(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
) -> Result<Option<ExistingItem>, StoreError> {
    let mut statement = transaction.prepare(
        "SELECT id,project_id,chat_id,turn_id,lifecycle,tool_state,activity_id,payload_digest,
                payload_bytes,revision FROM items WHERE session_id=?1 AND runtime_epoch=?2
         AND kind=?3 AND external_item_id=?4 LIMIT 2",
    )?;
    let rows = statement.query_map(
        params![
            command.session_id.as_str(),
            command.runtime_epoch.get() as i64,
            kind_text(command.payload.kind()),
            command.external_item_id.as_str()
        ],
        |r| {
            Ok(ExistingItem {
                item_id: r.get(0)?,
                project_id: r.get(1)?,
                chat_id: r.get(2)?,
                turn_id: r.get(3)?,
                lifecycle: r.get(4)?,
                tool_state: r.get(5)?,
                activity_id: r.get(6)?,
                payload_digest: r.get(7)?,
                payload_bytes: r.get(8)?,
                revision: r.get(9)?,
            })
        },
    )?;
    let mut matches = Vec::with_capacity(2);
    for row in rows {
        matches.push(row?);
    }
    if matches.len() > 1 {
        return Err(StoreError::Conflict {
            resource: "itemIdentity",
        });
    }
    Ok(matches.pop())
}

mod update;
use update::update_existing;

fn validate_lifecycle(command: &ItemIngressCommand) -> Result<(), StoreError> {
    let needs_tool = matches!(
        command.payload,
        NormalizedPayload::ToolCall { .. } | NormalizedPayload::PermissionRequest { .. }
    );
    if needs_tool != command.tool_state.is_some()
        || command
            .tool_state
            .is_some_and(|tool| tool.is_terminal() != command.lifecycle.is_terminal())
    {
        return Err(StoreError::InvalidTransition {
            resource: "itemShape",
        });
    }
    if matches!(command.payload, NormalizedPayload::PermissionRequest { .. }) {
        return Err(StoreError::InvalidTransition {
            resource: "permissionAtomicIngress",
        });
    }
    Ok(())
}
fn verify_turn_owner(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
) -> Result<(), StoreError> {
    let valid: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM turns t
         JOIN agent_sessions s ON s.id=t.session_id
         JOIN session_runtime_epochs e ON e.session_id=t.session_id
           AND e.runtime_epoch=t.runtime_epoch
         WHERE t.project_id=?1 AND t.chat_id=?2 AND t.session_id=?3 AND t.id=?4
           AND t.runtime_epoch=?5 AND t.state NOT IN (
             'completed','interrupted','cancelled','failed','deliveryUnknown'
           ) AND s.state IN ('ready','active') AND e.state='running')",
        params![
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.turn_id.as_str(),
            command.runtime_epoch.get() as i64
        ],
        |row| row.get(0),
    )?;
    if valid {
        Ok(())
    } else {
        Err(StoreError::Conflict {
            resource: "itemOwner",
        })
    }
}
fn duplicate(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
    existing: ExistingItem,
) -> Result<ItemIngressMutation, StoreError> {
    let projection: i64 = transaction.query_row(
        "SELECT projection_revision FROM agent_sessions WHERE id=?1",
        [command.session_id.as_str()],
        |r| r.get(0),
    )?;
    Ok(ItemIngressMutation {
        item_id: SageItemId::parse(existing.item_id).map_err(|_| StoreError::Corrupt("item id"))?,
        entity: EntityMutation {
            revision: entity_revision_from_i64(existing.revision)?,
            projection_revision: ProjectionRevision::new(projection as u64)
                .map_err(|_| StoreError::Corrupt("projection revision"))?,
            disposition: MutationDisposition::Duplicate,
        },
    })
}
fn kind_text(value: sage_acp_protocol::lifecycle::ItemKind) -> &'static str {
    value.as_str()
}
