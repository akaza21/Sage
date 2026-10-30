use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::lifecycle::{DeliveryState, TurnState, TurnTerminalOutcome, TurnTerminalReason};

use super::{
    MutationDisposition, SendObservation, StoreError, TurnMutation, TurnMutationCommand,
    facts::{
        append_fact, entity_revision_from_i64, next_projection_revision, projection_from_i64,
        publish_projection,
    },
    terminal_reason,
};

pub(crate) fn begin_send(
    connection: &mut rusqlite::Connection,
    command: TurnMutationCommand,
) -> Result<TurnMutation, StoreError> {
    let transaction = connection.transaction()?;
    let mutation = begin_send_in(&transaction, command)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(mutation)
}

pub(super) fn begin_send_in(
    transaction: &Transaction<'_>,
    command: TurnMutationCommand,
) -> Result<TurnMutation, StoreError> {
    transition_turn_in(
        transaction,
        command,
        TurnState::Sending,
        DeliveryState::Writing,
        "sendStarted",
    )
}

pub(crate) fn record_send_observation(
    connection: &mut rusqlite::Connection,
    command: TurnMutationCommand,
    observation: SendObservation,
) -> Result<TurnMutation, StoreError> {
    let transaction = connection.transaction()?;
    let mutation = record_send_observation_in(&transaction, command, observation, None)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(mutation)
}

pub(super) fn record_send_observation_in(
    transaction: &Transaction<'_>,
    command: TurnMutationCommand,
    observation: SendObservation,
    outcome: Option<TurnTerminalOutcome>,
) -> Result<TurnMutation, StoreError> {
    let (state, delivery) = current_turn(transaction, &command)?;
    let (next_state, next_delivery, fact) = match (observation, state, delivery) {
        (SendObservation::ProcessLost, TurnState::Failed, DeliveryState::NotWritten) => {
            (state, delivery, "sendFailedBeforeWrite")
        }
        (SendObservation::ProcessLost, TurnState::Failed, DeliveryState::ZeroBytes) => {
            (state, delivery, "sendFailedZeroBytes")
        }
        (
            SendObservation::ProcessLost,
            TurnState::DeliveryUnknown,
            DeliveryState::PossibleBytes | DeliveryState::CompleteBytes,
        ) => (state, delivery, "sendDeliveryUnknown"),
        (SendObservation::ProcessLost, TurnState::Interrupted, _) => {
            (state, delivery, "turnInterrupted")
        }
        (
            SendObservation::ProcessLost,
            TurnState::Cancelled,
            DeliveryState::NotWritten | DeliveryState::ZeroBytes,
        ) => (state, delivery, "turnCancelled"),
        (SendObservation::ProcessLost, TurnState::Queued, DeliveryState::NotWritten) => (
            TurnState::Cancelled,
            DeliveryState::NotWritten,
            "turnCancelled",
        ),
        (SendObservation::ZeroBytes, TurnState::Sending, DeliveryState::Writing) => (
            TurnState::Sending,
            DeliveryState::ZeroBytes,
            "sendZeroBytes",
        ),
        (SendObservation::PossibleBytes, TurnState::Sending, DeliveryState::Writing) => (
            TurnState::DeliveryUnknown,
            DeliveryState::PossibleBytes,
            "sendDeliveryUnknown",
        ),
        (SendObservation::CompleteBytes, TurnState::Sending, DeliveryState::Writing) => (
            TurnState::Sending,
            DeliveryState::CompleteBytes,
            "sendCompleteBytes",
        ),
        (
            SendObservation::Correlated,
            TurnState::Sending,
            DeliveryState::Writing | DeliveryState::PossibleBytes | DeliveryState::CompleteBytes,
        ) => (
            TurnState::Running,
            DeliveryState::Correlated,
            "sendCorrelated",
        ),
        (
            SendObservation::ProcessLost,
            TurnState::Sending,
            DeliveryState::NotWritten | DeliveryState::ZeroBytes,
        ) => (TurnState::Failed, delivery, "sendFailedZeroBytes"),
        (SendObservation::ProcessLost, TurnState::Sending, DeliveryState::Writing) => (
            TurnState::DeliveryUnknown,
            DeliveryState::PossibleBytes,
            "sendDeliveryUnknown",
        ),
        (
            SendObservation::ProcessLost,
            TurnState::Sending,
            DeliveryState::PossibleBytes | DeliveryState::CompleteBytes,
        ) => (TurnState::DeliveryUnknown, delivery, "sendDeliveryUnknown"),
        (
            SendObservation::ProcessLost,
            TurnState::Cancelling,
            DeliveryState::NotWritten | DeliveryState::ZeroBytes,
        ) => (TurnState::Cancelled, delivery, "turnCancelled"),
        (SendObservation::ProcessLost, TurnState::Cancelling, DeliveryState::Writing) => (
            TurnState::DeliveryUnknown,
            DeliveryState::PossibleBytes,
            "sendDeliveryUnknown",
        ),
        (
            SendObservation::ProcessLost,
            TurnState::Cancelling,
            DeliveryState::PossibleBytes | DeliveryState::CompleteBytes,
        ) => (TurnState::DeliveryUnknown, delivery, "sendDeliveryUnknown"),
        (SendObservation::ProcessLost, TurnState::Cancelling, DeliveryState::Correlated) => {
            (TurnState::Interrupted, delivery, "turnInterrupted")
        }
        (
            SendObservation::ProcessLost,
            TurnState::Running
            | TurnState::WaitingModel
            | TurnState::WaitingTool
            | TurnState::WaitingApproval
            | TurnState::WaitingUser,
            _,
        ) => (TurnState::Interrupted, delivery, "turnInterrupted"),
        _ => return Err(StoreError::InvalidTransition { resource: "send" }),
    };
    let mutation = transition_turn_in(
        transaction,
        command.clone(),
        next_state,
        next_delivery,
        fact,
    )?;
    if matches!(
        next_state,
        TurnState::Failed
            | TurnState::Cancelled
            | TurnState::Interrupted
            | TurnState::DeliveryUnknown
    ) && (mutation.disposition == MutationDisposition::Applied || outcome.is_some())
    {
        let outcome = outcome.unwrap_or_else(|| TurnTerminalReason::Other.into());
        terminal_reason::record_in(
            transaction,
            (&command).into(),
            outcome.reason,
            outcome.message.as_deref(),
            outcome.remediation.as_deref(),
            command.now_millis,
        )?;
    }
    Ok(mutation)
}

pub(crate) fn transition_turn(
    connection: &mut rusqlite::Connection,
    command: TurnMutationCommand,
    next_state: TurnState,
    next_delivery: DeliveryState,
    fact_kind: &'static str,
) -> Result<TurnMutation, StoreError> {
    let transaction = connection.transaction()?;
    let mutation = transition_turn_in(&transaction, command, next_state, next_delivery, fact_kind)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(mutation)
}

pub(super) fn transition_turn_in(
    transaction: &Transaction<'_>,
    command: TurnMutationCommand,
    next_state: TurnState,
    next_delivery: DeliveryState,
    fact_kind: &'static str,
) -> Result<TurnMutation, StoreError> {
    let (state_text, delivery_text, revision): (String, String, i64) = transaction
        .query_row(
            "SELECT state, delivery_state, revision FROM turns
             WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND runtime_epoch=?4 AND id=?5",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64,
                command.turn_id.as_str()
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(StoreError::Conflict { resource: "turn" })?;
    let current_state = parse_turn_state(&state_text)?;
    let current_delivery = parse_delivery_state(&delivery_text)?;
    if current_state.is_terminal() {
        if current_state == next_state && current_delivery == next_delivery {
            let projection: i64 = transaction.query_row(
                "SELECT projection_revision FROM agent_sessions WHERE id=?1",
                [command.session_id.as_str()],
                |row| row.get(0),
            )?;
            return Ok(TurnMutation {
                state: current_state,
                delivery_state: current_delivery,
                revision: entity_revision_from_i64(revision)?,
                projection_revision: projection_from_i64(projection)?,
                disposition: MutationDisposition::Duplicate,
            });
        }
        return Err(StoreError::TerminalConflict { resource: "turn" });
    }
    let expected = entity_revision_from_i64(revision)?;
    if expected != command.expected_revision {
        return Err(StoreError::Conflict {
            resource: "turnRevision",
        });
    }
    if current_state != next_state && !current_state.can_transition_to(next_state) {
        return Err(StoreError::InvalidTransition { resource: "turn" });
    }
    if !delivery_can_transition(current_delivery, next_delivery) {
        return Err(StoreError::InvalidTransition {
            resource: "delivery",
        });
    }
    let next_revision = expected.next().map_err(|_| StoreError::CounterExhausted)?;
    let projection = next_projection_revision(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    if current_delivery == DeliveryState::Writing && next_delivery == DeliveryState::Correlated {
        ensure_one(transaction.execute(
            "UPDATE turns SET delivery_state='possibleBytes' WHERE id=?1",
            [command.turn_id.as_str()],
        )?)?;
    }
    let terminal_at = next_state.is_terminal().then_some(command.now_millis);
    crate::acp::context_handoff::delivery::observe(
        transaction,
        &command,
        next_state,
        next_delivery,
        projection,
    )?;
    ensure_one(transaction.execute(
        "UPDATE turns SET state=?2, delivery_state=?3, revision=?4, updated_at=?5,
         terminal_at=?6 WHERE id=?1",
        params![
            command.turn_id.as_str(),
            turn_state_text(next_state),
            delivery_state_text(next_delivery),
            next_revision.get() as i64,
            command.now_millis,
            terminal_at
        ],
    )?)?;
    append_fact(
        transaction,
        &command,
        fact_kind,
        turn_state_text(next_state),
        command.turn_id.as_str(),
        projection,
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
        state: next_state,
        delivery_state: next_delivery,
        revision: next_revision,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}

fn current_turn(
    connection: &rusqlite::Connection,
    command: &TurnMutationCommand,
) -> Result<(TurnState, DeliveryState), StoreError> {
    let row: (String, String) = connection.query_row(
        "SELECT state,delivery_state FROM turns WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND runtime_epoch=?4 AND id=?5",
        params![command.project_id.as_str(), command.chat_id.as_str(), command.session_id.as_str(), command.runtime_epoch.get() as i64, command.turn_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?.ok_or(StoreError::Conflict { resource: "turn" })?;
    Ok((parse_turn_state(&row.0)?, parse_delivery_state(&row.1)?))
}

fn delivery_can_transition(old: DeliveryState, new: DeliveryState) -> bool {
    old == new
        || matches!(
            (old, new),
            (
                DeliveryState::NotWritten,
                DeliveryState::Writing | DeliveryState::ZeroBytes
            ) | (DeliveryState::ZeroBytes, DeliveryState::Writing)
                | (
                    DeliveryState::Writing,
                    DeliveryState::ZeroBytes
                        | DeliveryState::PossibleBytes
                        | DeliveryState::CompleteBytes
                        | DeliveryState::Correlated
                )
                | (
                    DeliveryState::PossibleBytes | DeliveryState::CompleteBytes,
                    DeliveryState::Correlated
                )
        )
}

fn ensure_one(changed: usize) -> Result<(), StoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(StoreError::Corrupt("turn"))
    }
}

pub(crate) fn parse_turn_state(value: &str) -> Result<TurnState, StoreError> {
    match value {
        "queued" => Ok(TurnState::Queued),
        "sending" => Ok(TurnState::Sending),
        "running" => Ok(TurnState::Running),
        "waitingModel" => Ok(TurnState::WaitingModel),
        "waitingTool" => Ok(TurnState::WaitingTool),
        "waitingApproval" => Ok(TurnState::WaitingApproval),
        "waitingUser" => Ok(TurnState::WaitingUser),
        "cancelling" => Ok(TurnState::Cancelling),
        "completed" => Ok(TurnState::Completed),
        "interrupted" => Ok(TurnState::Interrupted),
        "cancelled" => Ok(TurnState::Cancelled),
        "failed" => Ok(TurnState::Failed),
        "deliveryUnknown" => Ok(TurnState::DeliveryUnknown),
        _ => Err(StoreError::Corrupt("turn state")),
    }
}

pub(crate) fn turn_state_text(value: TurnState) -> &'static str {
    match value {
        TurnState::Queued => "queued",
        TurnState::Sending => "sending",
        TurnState::Running => "running",
        TurnState::WaitingModel => "waitingModel",
        TurnState::WaitingTool => "waitingTool",
        TurnState::WaitingApproval => "waitingApproval",
        TurnState::WaitingUser => "waitingUser",
        TurnState::Cancelling => "cancelling",
        TurnState::Completed => "completed",
        TurnState::Interrupted => "interrupted",
        TurnState::Cancelled => "cancelled",
        TurnState::Failed => "failed",
        TurnState::DeliveryUnknown => "deliveryUnknown",
    }
}

pub(crate) fn parse_delivery_state(value: &str) -> Result<DeliveryState, StoreError> {
    match value {
        "notWritten" => Ok(DeliveryState::NotWritten),
        "writing" => Ok(DeliveryState::Writing),
        "zeroBytes" => Ok(DeliveryState::ZeroBytes),
        "possibleBytes" => Ok(DeliveryState::PossibleBytes),
        "completeBytes" => Ok(DeliveryState::CompleteBytes),
        "correlated" => Ok(DeliveryState::Correlated),
        _ => Err(StoreError::Corrupt("delivery state")),
    }
}

fn delivery_state_text(value: DeliveryState) -> &'static str {
    match value {
        DeliveryState::NotWritten => "notWritten",
        DeliveryState::Writing => "writing",
        DeliveryState::ZeroBytes => "zeroBytes",
        DeliveryState::PossibleBytes => "possibleBytes",
        DeliveryState::CompleteBytes => "completeBytes",
        DeliveryState::Correlated => "correlated",
    }
}
