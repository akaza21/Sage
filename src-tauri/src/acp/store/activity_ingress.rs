use rusqlite::{Transaction, params};

use crate::acp::{
    contract::{
        EntityRevision, ProjectionRevision, RequiredText, RuntimeEpoch, SageActivityId,
        SageAgentSessionId, SageChatId, SageItemId, SageProjectId, SageTurnId,
    },
    intent::{ActivityProvenanceV1, ExternalId},
    lifecycle::ActivityLifecycle,
    limits::LIMITS,
    projection::ActivityRole,
};

use super::{
    EntityMutation, MutationDisposition, StoreError,
    facts::{
        append_fact_payload, entity_revision_from_i64, hex_digest, next_projection_revision,
        publish_projection,
    },
    state::activity_lifecycle_text,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivityIngressCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub runtime_epoch: RuntimeEpoch,
    pub external_activity_id: ExternalId,
    pub activity_kind: RequiredText<128>,
    pub parent_activity_id: Option<SageActivityId>,
    pub parent_item_id: SageItemId,
    pub role: ActivityRole,
    pub lifecycle: ActivityLifecycle,
    pub depth: u8,
    pub observed_sequence: u64,
    pub provenance: ActivityProvenanceV1,
    pub provenance_digest: String,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivityIngressMutation {
    pub activity_id: SageActivityId,
    pub entity: EntityMutation,
}

pub(crate) fn apply_activity_ingress(
    connection: &mut rusqlite::Connection,
    command: ActivityIngressCommand,
) -> Result<ActivityIngressMutation, StoreError> {
    command
        .provenance
        .validate_for_persistence()
        .map_err(|_| StoreError::Corrupt("activity provenance authority"))?;
    if command.depth > LIMITS.native_thread_projection.observed_depth as u8
        || command.observed_sequence == 0
    {
        return Err(StoreError::InvalidTransition {
            resource: "activityShape",
        });
    }
    let observed_sequence =
        i64::try_from(command.observed_sequence).map_err(|_| StoreError::CounterExhausted)?;
    let provenance = serde_json::to_string(&command.provenance)
        .map_err(|_| StoreError::Corrupt("activity provenance"))?;
    if provenance.len() > LIMITS.wire.metadata_bytes as usize
        || hex_digest(provenance.as_bytes()) != command.provenance_digest
    {
        return Err(StoreError::Corrupt("activity provenance digest"));
    }
    let transaction = connection.transaction()?;
    verify_owner(&transaction, &command)?;
    if let Some(existing) = existing_activity(&transaction, &command)? {
        let result = update_existing(&transaction, command, existing, provenance)?;
        transaction
            .commit()
            .map_err(|_| StoreError::CommitUnknown)?;
        return Ok(result);
    }
    let count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM session_activities WHERE turn_id=?1",
        [command.turn_id.as_str()],
        |r| r.get(0),
    )?;
    if count >= LIMITS.native_thread_projection.activity_groups_per_turn as i64 {
        return Err(StoreError::Capacity {
            resource: "turnActivities",
            limit: LIMITS.native_thread_projection.activity_groups_per_turn,
        });
    }
    let maximum: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(observed_sequence),0) FROM session_activities WHERE turn_id=?1",
        [command.turn_id.as_str()],
        |r| r.get(0),
    )?;
    if maximum >= observed_sequence {
        return Err(StoreError::Conflict {
            resource: "activitySequence",
        });
    }
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    let activity_id = SageActivityId::new();
    let terminal = command
        .lifecycle
        .is_terminal()
        .then_some(command.now_millis);
    transaction.execute(
        "INSERT INTO session_activities(id,project_id,chat_id,session_id,turn_id,runtime_epoch,
         external_activity_id,activity_kind,parent_activity_id,parent_item_id,role,lifecycle,depth,
         observed_sequence,provenance_json,revision,created_at,updated_at,terminal_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,0,?16,?16,?17)",
        params![
            activity_id.as_str(),
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.turn_id.as_str(),
            command.runtime_epoch.get() as i64,
            command.external_activity_id.as_str(),
            command.activity_kind.as_str(),
            command
                .parent_activity_id
                .as_ref()
                .map(|value| value.as_str()),
            command.parent_item_id.as_str(),
            role_text(command.role),
            activity_lifecycle_text(command.lifecycle),
            command.depth as i64,
            observed_sequence,
            provenance,
            command.now_millis,
            terminal
        ],
    )?;
    append_activity_fact(&transaction, &command, &activity_id, projection)?;
    publish_projection(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        projection,
        command.now_millis,
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(ActivityIngressMutation {
        activity_id,
        entity: EntityMutation {
            revision: EntityRevision::ZERO,
            projection_revision: projection,
            disposition: MutationDisposition::Applied,
        },
    })
}

struct ExistingActivity {
    id: String,
    project_id: String,
    chat_id: String,
    turn_id: String,
    parent_activity: Option<String>,
    parent_item: String,
    role: String,
    lifecycle: String,
    depth: i64,
    sequence: i64,
    provenance: String,
    revision: i64,
}
fn existing_activity(
    transaction: &Transaction<'_>,
    command: &ActivityIngressCommand,
) -> Result<Option<ExistingActivity>, StoreError> {
    let mut statement = transaction.prepare(
        "SELECT id,project_id,chat_id,turn_id,parent_activity_id,parent_item_id,role,lifecycle,
                depth,observed_sequence,provenance_json,revision FROM session_activities
         WHERE session_id=?1 AND runtime_epoch=?2 AND activity_kind=?3
           AND external_activity_id=?4 LIMIT 2",
    )?;
    let rows = statement.query_map(
        params![
            command.session_id.as_str(),
            command.runtime_epoch.get() as i64,
            command.activity_kind.as_str(),
            command.external_activity_id.as_str()
        ],
        |r| {
            Ok(ExistingActivity {
                id: r.get(0)?,
                project_id: r.get(1)?,
                chat_id: r.get(2)?,
                turn_id: r.get(3)?,
                parent_activity: r.get(4)?,
                parent_item: r.get(5)?,
                role: r.get(6)?,
                lifecycle: r.get(7)?,
                depth: r.get(8)?,
                sequence: r.get(9)?,
                provenance: r.get(10)?,
                revision: r.get(11)?,
            })
        },
    )?;
    let mut matches = Vec::with_capacity(2);
    for row in rows {
        matches.push(row?);
    }
    if matches.len() > 1 {
        return Err(StoreError::Conflict {
            resource: "activityIdentity",
        });
    }
    Ok(matches.pop())
}
fn update_existing(
    transaction: &Transaction<'_>,
    command: ActivityIngressCommand,
    existing: ExistingActivity,
    provenance: String,
) -> Result<ActivityIngressMutation, StoreError> {
    if existing.project_id != command.project_id.as_str()
        || existing.chat_id != command.chat_id.as_str()
        || existing.turn_id != command.turn_id.as_str()
        || existing.parent_activity.as_deref()
            != command.parent_activity_id.as_ref().map(|v| v.as_str())
        || existing.parent_item != command.parent_item_id.as_str()
        || existing.role != role_text(command.role)
        || existing.depth != command.depth as i64
        || existing.sequence != command.observed_sequence as i64
        || existing.provenance != provenance
    {
        return Err(StoreError::Conflict {
            resource: "activityIdentity",
        });
    }
    let current = super::state::parse_activity_lifecycle(&existing.lifecycle)?;
    if current == command.lifecycle {
        return duplicate(transaction, &command, existing);
    }
    if current.is_terminal() {
        return Err(StoreError::TerminalConflict {
            resource: "activity",
        });
    }
    if !current.can_transition_to(command.lifecycle) {
        return Err(StoreError::InvalidTransition {
            resource: "activity",
        });
    }
    let next = entity_revision_from_i64(existing.revision)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    let projection = next_projection_revision(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    let terminal = command
        .lifecycle
        .is_terminal()
        .then_some(command.now_millis);
    transaction.execute(
        "UPDATE session_activities SET lifecycle=?2,revision=?3,updated_at=?4,terminal_at=?5
         WHERE id=?1",
        params![
            existing.id,
            activity_lifecycle_text(command.lifecycle),
            next.get() as i64,
            command.now_millis,
            terminal
        ],
    )?;
    let activity_id =
        SageActivityId::parse(existing.id).map_err(|_| StoreError::Corrupt("activity id"))?;
    append_activity_fact(transaction, &command, &activity_id, projection)?;
    publish_projection(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        projection,
        command.now_millis,
    )?;
    Ok(ActivityIngressMutation {
        activity_id,
        entity: EntityMutation {
            revision: next,
            projection_revision: projection,
            disposition: MutationDisposition::Applied,
        },
    })
}
fn verify_owner(
    transaction: &Transaction<'_>,
    command: &ActivityIngressCommand,
) -> Result<(), StoreError> {
    let valid: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM turns t JOIN items i ON i.id=?6
         JOIN agent_sessions s ON s.id=t.session_id
         JOIN session_runtime_epochs e ON e.session_id=t.session_id
           AND e.runtime_epoch=t.runtime_epoch
         WHERE t.project_id=?1 AND t.chat_id=?2 AND t.session_id=?3 AND t.id=?4
           AND t.runtime_epoch=?5 AND t.state NOT IN (
             'completed','interrupted','cancelled','failed','deliveryUnknown'
           ) AND s.state IN ('ready','active') AND e.state='running'
           AND i.project_id=t.project_id AND i.chat_id=t.chat_id
           AND i.session_id=t.session_id AND i.turn_id=t.id AND i.runtime_epoch=t.runtime_epoch)",
        params![
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.turn_id.as_str(),
            command.runtime_epoch.get() as i64,
            command.parent_item_id.as_str()
        ],
        |row| row.get(0),
    )?;
    if !valid {
        return Err(StoreError::Conflict {
            resource: "activityOwner",
        });
    }
    if let Some(parent) = &command.parent_activity_id {
        let parent_ok: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_activities WHERE id=?1 AND project_id=?2
             AND chat_id=?3 AND session_id=?4 AND turn_id=?5 AND runtime_epoch=?6)",
            params![
                parent.as_str(),
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.turn_id.as_str(),
                command.runtime_epoch.get() as i64
            ],
            |row| row.get(0),
        )?;
        if !parent_ok {
            return Err(StoreError::Conflict {
                resource: "parentActivity",
            });
        }
    }
    Ok(())
}
fn append_activity_fact(
    transaction: &Transaction<'_>,
    command: &ActivityIngressCommand,
    activity: &SageActivityId,
    projection: ProjectionRevision,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(&serde_json::json!({
        "kind": "activityIngress",
        "entityId": activity.as_str(),
        "state": activity_lifecycle_text(command.lifecycle)
    }))
    .map_err(|_| StoreError::Corrupt("activity fact"))?;
    append_fact_payload(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        Some(command.turn_id.as_str()),
        "activityIngress",
        &payload,
        projection,
        command.now_millis,
    )
}
fn duplicate(
    transaction: &Transaction<'_>,
    command: &ActivityIngressCommand,
    existing: ExistingActivity,
) -> Result<ActivityIngressMutation, StoreError> {
    let projection: i64 = transaction.query_row(
        "SELECT projection_revision FROM agent_sessions WHERE id=?1",
        [command.session_id.as_str()],
        |r| r.get(0),
    )?;
    Ok(ActivityIngressMutation {
        activity_id: SageActivityId::parse(existing.id)
            .map_err(|_| StoreError::Corrupt("activity id"))?,
        entity: EntityMutation {
            revision: entity_revision_from_i64(existing.revision)?,
            projection_revision: ProjectionRevision::new(projection as u64)
                .map_err(|_| StoreError::Corrupt("projection revision"))?,
            disposition: MutationDisposition::Duplicate,
        },
    })
}
fn role_text(role: ActivityRole) -> &'static str {
    match role {
        ActivityRole::Root => "root",
        ActivityRole::Fork => "fork",
        ActivityRole::Child => "child",
        ActivityRole::Unknown => "unknown",
    }
}
