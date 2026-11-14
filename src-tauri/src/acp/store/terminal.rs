use rusqlite::Transaction;
#[cfg(test)]
use rusqlite::{OptionalExtension, params};

use sage_acp_protocol::contract::{EntityRevision, ProjectionRevision};
#[cfg(test)]
use sage_acp_protocol::lifecycle::classify_terminal;

#[cfg(test)]
use super::SessionTerminalCommand;
#[cfg(test)]
use super::facts::next_projection_revision;
#[cfg(test)]
use super::state::{parse_session_state, session_state_text};
use super::{
    EntityMutation, FactPayload, MutationDisposition, StoreError, TurnMutationCommand,
    facts::{append_fact_payload, entity_revision_from_i64, projection_from_i64},
};

#[cfg(test)]
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
            sage_acp_protocol::lifecycle::TerminalUpdate::NoOp => Ok(EntityMutation {
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
    terminal: sage_acp_protocol::lifecycle::TurnState,
    delivery: sage_acp_protocol::lifecycle::DeliveryState,
) -> Result<EntityMutation, StoreError> {
    super::send::transition_turn(connection, command, terminal, delivery, "turnTerminal").map(
        |value| EntityMutation {
            revision: value.revision,
            projection_revision: value.projection_revision,
            disposition: value.disposition,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_entity_fact(
    transaction: &Transaction<'_>,
    project: &sage_acp_protocol::contract::SageProjectId,
    chat: &sage_acp_protocol::contract::SageChatId,
    session: &sage_acp_protocol::contract::SageAgentSessionId,
    turn: Option<&sage_acp_protocol::contract::SageTurnId>,
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
pub(super) fn ensure_one(changed: usize, resource: &'static str) -> Result<(), StoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(StoreError::Corrupt(resource))
    }
}
