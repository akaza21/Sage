use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::{
    contract::{EntityRevision, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId},
    lifecycle::TurnState,
};

use super::{
    BeginCancelCommand, MutationDisposition, StoreError, TurnMutation,
    facts::{append_fact_payload, entity_revision_from_i64, projection_from_i64},
    send::{parse_delivery_state, parse_turn_state, transition_turn_in},
};

pub(crate) fn begin_cancel(
    connection: &mut rusqlite::Connection,
    command: BeginCancelCommand,
) -> Result<TurnMutation, StoreError> {
    let transaction = connection.transaction()?;
    let mutation = begin_cancel_in(&transaction, command)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(mutation)
}

/// Whether the turn has settled, read from its own row rather than a chat page.
/// A turn missing from the session reads as not terminal.
pub(crate) fn turn_is_terminal(
    connection: &rusqlite::Connection,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    session_id: &SageAgentSessionId,
    turn_id: &SageTurnId,
) -> Result<bool, StoreError> {
    let state: Option<String> = connection
        .query_row(
            "SELECT state FROM turns
             WHERE id=?1 AND project_id=?2 AND chat_id=?3 AND session_id=?4",
            params![
                turn_id.as_str(),
                project_id.as_str(),
                chat_id.as_str(),
                session_id.as_str()
            ],
            |row| row.get(0),
        )
        .optional()?;
    Ok(state
        .as_deref()
        .map(parse_turn_state)
        .transpose()?
        .is_some_and(TurnState::is_terminal))
}

pub(super) fn begin_cancel_in(
    transaction: &Transaction<'_>,
    command: BeginCancelCommand,
) -> Result<TurnMutation, StoreError> {
    let turn = &command.turn;
    let (state_text, delivery_text, revision): (String, String, i64) = transaction
        .query_row(
            "SELECT state,delivery_state,revision FROM turns
             WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND runtime_epoch=?4 AND id=?5",
            params![
                turn.project_id.as_str(),
                turn.chat_id.as_str(),
                turn.session_id.as_str(),
                turn.runtime_epoch.get() as i64,
                turn.turn_id.as_str()
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(StoreError::Conflict { resource: "turn" })?;
    let state = parse_turn_state(&state_text)?;
    let delivery = parse_delivery_state(&delivery_text)?;
    let revision = entity_revision_from_i64(revision)?;
    if state == TurnState::Cancelling {
        let (original_revision, resulting_revision) =
            cancel_intent_revisions(transaction, &command)?;
        if resulting_revision != revision {
            return Err(StoreError::Corrupt("turn cancel intent outcome"));
        }
        if turn.expected_revision != revision && turn.expected_revision != original_revision {
            return Err(StoreError::Conflict {
                resource: "turnRevision",
            });
        }
        let projection: i64 = transaction.query_row(
            "SELECT projection_revision FROM agent_sessions WHERE id=?1",
            [turn.session_id.as_str()],
            |row| row.get(0),
        )?;
        return Ok(TurnMutation {
            state,
            delivery_state: delivery,
            revision,
            projection_revision: projection_from_i64(projection)?,
            disposition: MutationDisposition::Duplicate,
        });
    }
    if state.is_terminal() {
        return Err(StoreError::TerminalConflict { resource: "turn" });
    }
    if revision != turn.expected_revision {
        return Err(StoreError::Conflict {
            resource: "turnRevision",
        });
    }
    if !state.can_transition_to(TurnState::Cancelling) {
        return Err(StoreError::InvalidTransition {
            resource: "turnCancel",
        });
    }
    let mutation = transition_turn_in(
        transaction,
        command.turn.clone(),
        TurnState::Cancelling,
        delivery,
        "turnCancelling",
    )?;
    append_cancel_intent(transaction, &command, &mutation)?;
    Ok(mutation)
}

fn append_cancel_intent(
    transaction: &Transaction<'_>,
    command: &BeginCancelCommand,
    mutation: &TurnMutation,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(&serde_json::json!({
        "kind": "turnCancelIntent",
        "entityId": command.turn.turn_id.as_str(),
        "state": "cancelling",
        "clientIntentId": command.client_intent_id.as_str(),
        "expectedRevision": command.turn.expected_revision.get().to_string(),
        "resultingRevision": mutation.revision.get().to_string(),
    }))
    .map_err(|_| StoreError::Corrupt("turn cancel intent serialization"))?;
    append_fact_payload(
        transaction,
        command.turn.project_id.as_str(),
        command.turn.chat_id.as_str(),
        command.turn.session_id.as_str(),
        Some(command.turn.turn_id.as_str()),
        "turnCancelIntent",
        &payload,
        mutation.projection_revision,
        command.turn.now_millis,
    )
}

fn cancel_intent_revisions(
    transaction: &Transaction<'_>,
    command: &BeginCancelCommand,
) -> Result<(EntityRevision, EntityRevision), StoreError> {
    let row: (i64, Option<String>, Option<String>) = transaction.query_row(
        "SELECT COUNT(*), MIN(json_extract(payload_json,'$.expectedRevision')),
                MIN(json_extract(payload_json,'$.resultingRevision'))
         FROM session_facts
         WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4
           AND kind='turnCancelIntent'
           AND json_extract(payload_json,'$.clientIntentId')=?5",
        params![
            command.turn.project_id.as_str(),
            command.turn.chat_id.as_str(),
            command.turn.session_id.as_str(),
            command.turn.turn_id.as_str(),
            command.client_intent_id.as_str()
        ],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    match row {
        (1, Some(expected), Some(resulting)) => {
            Ok((parse_revision(expected)?, parse_revision(resulting)?))
        }
        (0, _, _) => Err(StoreError::Conflict {
            resource: "cancelIntent",
        }),
        _ => Err(StoreError::Corrupt("turn cancel intent")),
    }
}

fn parse_revision(value: String) -> Result<EntityRevision, StoreError> {
    EntityRevision::new(
        value
            .parse::<u64>()
            .map_err(|_| StoreError::Corrupt("turn cancel intent revision"))?,
    )
    .map_err(|_| StoreError::Corrupt("turn cancel intent revision"))
}
