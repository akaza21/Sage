use rusqlite::{OptionalExtension, Transaction, params};

use sage_acp_protocol::{
    contract::EntityRevision,
    lifecycle::{DeliveryState, TurnState},
};

use super::{
    MutationDisposition, StoreError, TurnMutation, TurnMutationCommand,
    commands::{
        TurnFootprint, ensure_one, ensure_repository_unleased, ensure_turn_capacity,
        require_current_runtime,
    },
    facts::{
        append_fact, entity_revision_from_i64, next_projection_revision, projection_from_i64,
        publish_projection,
    },
};

/// SQL predicate: the Turn whose session and id are the given columns
/// answers a prompt, so its first item is the user's. A Turn the harness
/// started on its own has no prompt.
macro_rules! prompted_turn {
    ($session:literal, $turn:literal) => {
        concat!(
            "NOT EXISTS(SELECT 1 FROM session_facts started WHERE started.session_id=",
            $session,
            " AND started.turn_id=",
            $turn,
            " AND started.kind='harnessTurnStarted')"
        )
    };
}
pub(crate) use prompted_turn;

/// Opens a Turn the harness started on its own: no prompt item, already
/// running, and correlated, since the harness itself named it. The Turn is its
/// own client intent.
pub(crate) fn open_harness_turn(
    connection: &mut rusqlite::Connection,
    command: &TurnMutationCommand,
) -> Result<TurnMutation, StoreError> {
    let transaction = connection.transaction()?;
    let mutation = open_harness_turn_in(&transaction, command)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(mutation)
}

fn open_harness_turn_in(
    transaction: &Transaction<'_>,
    command: &TurnMutationCommand,
) -> Result<TurnMutation, StoreError> {
    if let Some(existing) = existing_turn(transaction, command)? {
        return Ok(existing);
    }
    if command.expected_revision != EntityRevision::ZERO {
        return Err(StoreError::Conflict {
            resource: "turnRevision",
        });
    }
    require_current_runtime(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        command.runtime_epoch,
    )?;
    // Recording the Turn cannot stop an out-of-process harness, but declining
    // it here makes the runner cancel it instead of adopting it mid-switch.
    ensure_repository_unleased(transaction, command.project_id.as_str())?;
    ensure_turn_capacity(
        transaction,
        command.project_id.as_str(),
        command.session_id.as_str(),
        TurnFootprint {
            items: 0,
            item_bytes: 0,
        },
    )?;
    let ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM turns WHERE session_id=?1",
        [command.session_id.as_str()],
        |row| row.get(0),
    )?;
    if ordinal < 0 {
        return Err(StoreError::CounterExhausted);
    }
    let projection = next_projection_revision(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    transaction.execute(
        "INSERT INTO turns (id,project_id,chat_id,session_id,runtime_epoch,ordinal,
         client_intent_id,state,delivery_state,revision,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?1,'running','correlated',0,?7,?7)",
        params![
            command.turn_id.as_str(),
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.runtime_epoch.get() as i64,
            ordinal,
            command.now_millis
        ],
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_session_usage SET turn_count=turn_count+1,revision=revision+1 WHERE session_id=?1",
            [command.session_id.as_str()],
        )?,
        "session usage",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_project_usage SET turn_count=turn_count+1,revision=revision+1 WHERE project_id=?1",
            [command.project_id.as_str()],
        )?,
        "project usage",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_store_usage SET turn_count=turn_count+1,revision=revision+1 WHERE singleton=1",
            [],
        )?,
        "global usage",
    )?;
    append_fact(
        transaction,
        command,
        "harnessTurnStarted",
        "running",
        command.turn_id.as_str(),
        projection,
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state='active',revision=revision+1,updated_at=?2 WHERE id=?1",
            params![command.session_id.as_str(), command.now_millis],
        )?,
        "session",
    )?;
    publish_projection(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        projection,
        command.now_millis,
    )?;
    Ok(TurnMutation {
        state: TurnState::Running,
        delivery_state: DeliveryState::Correlated,
        revision: EntityRevision::ZERO,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}

fn existing_turn(
    transaction: &Transaction<'_>,
    command: &TurnMutationCommand,
) -> Result<Option<TurnMutation>, StoreError> {
    let row: Option<(String, String, String, i64, i64)> = transaction
        .query_row(
            "SELECT t.session_id,t.state,t.delivery_state,t.revision,s.projection_revision
             FROM turns t JOIN agent_sessions s ON s.id=t.session_id WHERE t.id=?1",
            [command.turn_id.as_str()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((session, state, delivery, revision, projection)) = row else {
        return Ok(None);
    };
    if session != command.session_id.as_str() {
        return Err(StoreError::Conflict { resource: "turn" });
    }
    Ok(Some(TurnMutation {
        state: super::send::parse_turn_state(&state)?,
        delivery_state: super::send::parse_delivery_state(&delivery)?,
        revision: entity_revision_from_i64(revision)?,
        projection_revision: projection_from_i64(projection)?,
        disposition: MutationDisposition::Duplicate,
    }))
}
