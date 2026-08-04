use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::{
    contract::{EntityRevision, ProjectionRevision},
    lifecycle::classify_terminal,
};

use super::{
    ActivityTerminalCommand, EntityMutation, FactPayload, ItemTerminalCommand, MutationDisposition,
    SessionTerminalCommand, StoreError, TurnMutationCommand,
    facts::{
        append_fact_payload, entity_revision_from_i64, next_projection_revision,
        projection_from_i64, publish_projection,
    },
    state::{
        activity_lifecycle_text, item_lifecycle_text, parse_activity_lifecycle,
        parse_item_lifecycle, parse_session_state, parse_tool_state, session_state_text,
        tool_state_text,
    },
};

pub(crate) fn apply_session_terminal(
    connection: &mut rusqlite::Connection,
    command: SessionTerminalCommand,
) -> Result<EntityMutation, StoreError> {
    if !command.terminal.is_terminal() {
        return Err(StoreError::InvalidTransition {
            resource: "session",
        });
    }
    let transaction = connection.transaction()?;
    let row:(String,i64,i64)=transaction.query_row(
        "SELECT state,revision,projection_revision FROM agent_sessions WHERE project_id=?1 AND chat_id=?2 AND id=?3",
        params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str()],
        |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
    ).optional()?.ok_or(StoreError::Conflict { resource:"session" })?;
    let current = parse_session_state(&row.0)?;
    if current.is_terminal() {
        return match classify_terminal(Some(&current), &command.terminal) {
            crate::acp::lifecycle::TerminalUpdate::NoOp => Ok(EntityMutation {
                revision: entity_revision_from_i64(row.1)?,
                projection_revision: projection_from_i64(row.2)?,
                disposition: MutationDisposition::Duplicate,
            }),
            _ => Err(StoreError::TerminalConflict {
                resource: "session",
            }),
        };
    }
    if !current.can_transition_to(command.terminal) {
        return Err(StoreError::InvalidTransition {
            resource: "session",
        });
    }
    let revision = entity_revision_from_i64(row.1)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    transaction.execute(
        "DELETE FROM chat_session_heads WHERE session_id=?1",
        [command.session_id.as_str()],
    )?;
    ensure_one(transaction.execute(
        "UPDATE agent_sessions SET state=?2,residency='cold',revision=?3,projection_revision=?4,updated_at=?5,terminal_at=?5 WHERE id=?1",
        params![command.session_id.as_str(),session_state_text(command.terminal),revision.get() as i64,projection.get() as i64,command.now_millis],
    )?,"session")?;
    append_entity_fact(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        None,
        "sessionTerminal",
        command.session_id.as_str(),
        session_state_text(command.terminal),
        projection,
        command.now_millis,
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE chats SET projection_revision=?3,updated_at=?4 WHERE project_id=?1 AND id=?2",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                projection.get() as i64,
                command.now_millis
            ],
        )?,
        "chat",
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(applied(revision, projection))
}

pub(crate) fn apply_turn_terminal(
    connection: &mut rusqlite::Connection,
    command: TurnMutationCommand,
    terminal: crate::acp::lifecycle::TurnState,
    delivery: crate::acp::lifecycle::DeliveryState,
) -> Result<EntityMutation, StoreError> {
    super::send::transition_turn(connection, command, terminal, delivery, "turnTerminal").map(
        |value| EntityMutation {
            revision: value.revision,
            projection_revision: value.projection_revision,
            disposition: value.disposition,
        },
    )
}

pub(crate) fn apply_item_terminal(
    connection: &mut rusqlite::Connection,
    command: ItemTerminalCommand,
) -> Result<EntityMutation, StoreError> {
    if !command.lifecycle.is_terminal()
        || command.tool_state.is_some_and(|state| !state.is_terminal())
        || !valid_digest(&command.payload_digest)
    {
        return Err(StoreError::InvalidTransition { resource: "item" });
    }
    let transaction = connection.transaction()?;
    let row:(String,Option<String>,String,i64)=transaction.query_row(
        "SELECT lifecycle,tool_state,payload_digest,revision FROM items WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4 AND runtime_epoch=?5 AND id=?6",
        params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str(),command.turn_id.as_str(),command.runtime_epoch.get() as i64,command.item_id.as_str()],
        |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).optional()?.ok_or(StoreError::Conflict { resource:"item" })?;
    let current = parse_item_lifecycle(&row.0)?;
    let current_tool = row.1.as_deref().map(parse_tool_state).transpose()?;
    if current.is_terminal() {
        if current == command.lifecycle
            && current_tool == command.tool_state
            && row.2 == command.payload_digest
        {
            return duplicate(&transaction, command.session_id.as_str(), row.3);
        }
        return Err(StoreError::TerminalConflict { resource: "item" });
    }
    if !current.can_transition_to(command.lifecycle) || row.2 != command.payload_digest {
        return Err(StoreError::InvalidTransition { resource: "item" });
    }
    if let (Some(old), Some(next)) = (current_tool, command.tool_state)
        && !old.can_transition_to(next)
    {
        return Err(StoreError::InvalidTransition { resource: "tool" });
    }
    let revision = entity_revision_from_i64(row.3)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    ensure_one(transaction.execute(
        "UPDATE items SET lifecycle=?2,tool_state=?3,revision=?4,updated_at=?5,terminal_at=?5 WHERE id=?1",
        params![command.item_id.as_str(),item_lifecycle_text(command.lifecycle),command.tool_state.map(tool_state_text),revision.get() as i64,command.now_millis],
    )?,"item")?;
    append_entity_fact(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        Some(&command.turn_id),
        "itemTerminal",
        command.item_id.as_str(),
        item_lifecycle_text(command.lifecycle),
        projection,
        command.now_millis,
    )?;
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
    Ok(applied(revision, projection))
}

pub(crate) fn apply_activity_terminal(
    connection: &mut rusqlite::Connection,
    command: ActivityTerminalCommand,
) -> Result<EntityMutation, StoreError> {
    if !command.lifecycle.is_terminal() {
        return Err(StoreError::InvalidTransition {
            resource: "activity",
        });
    }
    let transaction = connection.transaction()?;
    let row:(String,i64)=transaction.query_row(
        "SELECT lifecycle,revision FROM session_activities WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4 AND runtime_epoch=?5 AND id=?6",
        params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str(),command.turn_id.as_str(),command.runtime_epoch.get() as i64,command.activity_id.as_str()],
        |row|Ok((row.get(0)?,row.get(1)?)),
    ).optional()?.ok_or(StoreError::Conflict { resource:"activity" })?;
    let current = parse_activity_lifecycle(&row.0)?;
    if current.is_terminal() {
        return if current == command.lifecycle {
            duplicate(&transaction, command.session_id.as_str(), row.1)
        } else {
            Err(StoreError::TerminalConflict {
                resource: "activity",
            })
        };
    }
    if !current.can_transition_to(command.lifecycle) {
        return Err(StoreError::InvalidTransition {
            resource: "activity",
        });
    }
    let revision = entity_revision_from_i64(row.1)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    ensure_one(transaction.execute(
        "UPDATE session_activities SET lifecycle=?2,revision=?3,updated_at=?4,terminal_at=?4 WHERE id=?1",
        params![command.activity_id.as_str(),activity_lifecycle_text(command.lifecycle),revision.get() as i64,command.now_millis],
    )?,"activity")?;
    append_entity_fact(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        Some(&command.turn_id),
        "activityTerminal",
        command.activity_id.as_str(),
        activity_lifecycle_text(command.lifecycle),
        projection,
        command.now_millis,
    )?;
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
    Ok(applied(revision, projection))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_entity_fact(
    transaction: &Transaction<'_>,
    project: &crate::acp::contract::SageProjectId,
    chat: &crate::acp::contract::SageChatId,
    session: &crate::acp::contract::SageAgentSessionId,
    turn: Option<&crate::acp::contract::SageTurnId>,
    kind: &'static str,
    entity: &str,
    state: &'static str,
    projection: ProjectionRevision,
    now: i64,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(&FactPayload {
        kind: kind.into(),
        entity_id: entity.into(),
        state: state.into(),
    })
    .map_err(|_| StoreError::Corrupt("fact serialization"))?;
    append_fact_payload(
        transaction,
        project.as_str(),
        chat.as_str(),
        session.as_str(),
        turn.map(|value| value.as_str()),
        kind,
        &payload,
        projection,
        now,
    )
}

pub(super) fn duplicate(
    transaction: &Transaction<'_>,
    session: &str,
    revision: i64,
) -> Result<EntityMutation, StoreError> {
    let projection: i64 = transaction.query_row(
        "SELECT projection_revision FROM agent_sessions WHERE id=?1",
        [session],
        |row| row.get(0),
    )?;
    Ok(EntityMutation {
        revision: entity_revision_from_i64(revision)?,
        projection_revision: projection_from_i64(projection)?,
        disposition: MutationDisposition::Duplicate,
    })
}
pub(super) fn applied(
    revision: EntityRevision,
    projection_revision: ProjectionRevision,
) -> EntityMutation {
    EntityMutation {
        revision,
        projection_revision,
        disposition: MutationDisposition::Applied,
    }
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
pub(super) fn ensure_one(changed: usize, resource: &'static str) -> Result<(), StoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(StoreError::Corrupt(resource))
    }
}
