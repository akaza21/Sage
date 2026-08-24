use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::{
    contract::EntityRevision,
    lifecycle::{DeliveryState, TurnState},
};

use super::super::{
    AdapterStoreOwner, AdapterTurnCursor, MutationDisposition, StoreError, TurnMutation,
    TurnMutationCommand,
    facts::{entity_revision_from_i64, projection_from_i64},
    send::{parse_delivery_state, parse_turn_state, transition_turn_in},
};

pub(super) fn apply(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    next: TurnState,
    now_millis: i64,
) -> Result<TurnMutation, StoreError> {
    let (state, delivery, revision) = current(transaction, owner)?;
    if state.is_terminal() {
        return if state == next {
            duplicate(transaction, owner, state, delivery, revision)
        } else {
            Err(StoreError::TerminalConflict { resource: "turn" })
        };
    }
    if state == next {
        return duplicate(transaction, owner, state, delivery, revision);
    }
    if state == TurnState::Cancelling
        && matches!(
            next,
            TurnState::Running
                | TurnState::WaitingModel
                | TurnState::WaitingTool
                | TurnState::WaitingApproval
                | TurnState::WaitingUser
        )
    {
        return duplicate(transaction, owner, state, delivery, revision);
    }
    let next_delivery = match next {
        TurnState::Running if state == TurnState::Sending => match delivery {
            DeliveryState::Writing
            | DeliveryState::PossibleBytes
            | DeliveryState::CompleteBytes => DeliveryState::Correlated,
            _ => {
                return Err(StoreError::InvalidTransition {
                    resource: "adapterCorrelation",
                });
            }
        },
        TurnState::Cancelled | TurnState::Interrupted | TurnState::Failed
            if matches!(state, TurnState::Cancelling | TurnState::Sending) =>
        {
            delivery
        }
        TurnState::Running
        | TurnState::WaitingModel
        | TurnState::WaitingTool
        | TurnState::WaitingApproval
        | TurnState::WaitingUser
        | TurnState::Completed
        | TurnState::Interrupted
        | TurnState::Cancelled
        | TurnState::Failed
            if delivery == DeliveryState::Correlated =>
        {
            delivery
        }
        _ => {
            return Err(StoreError::InvalidTransition {
                resource: "adapterTurnState",
            });
        }
    };
    transition_turn_in(
        transaction,
        TurnMutationCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            turn_id: owner.turn_id.clone(),
            expected_revision: entity_revision_from_i64(revision)?,
            now_millis,
        },
        next,
        next_delivery,
        if next.is_terminal() {
            "turnTerminal"
        } else {
            "turnRuntimeState"
        },
    )
}

pub(super) fn cursor(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
) -> Result<AdapterTurnCursor, StoreError> {
    let (state, delivery_state, revision) = current(transaction, owner)?;
    let projection: i64 = transaction.query_row(
        "SELECT projection_revision FROM agent_sessions WHERE id=?1",
        [owner.session_id.as_str()],
        |row| row.get(0),
    )?;
    Ok(AdapterTurnCursor {
        turn_id: owner.turn_id.clone(),
        state,
        delivery_state,
        revision: entity_revision_from_i64(revision)?,
        projection_revision: projection_from_i64(projection)?,
    })
}

fn current(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
) -> Result<(TurnState, DeliveryState, i64), StoreError> {
    let row: (String, String, i64) = transaction
        .query_row(
            "SELECT state,delivery_state,revision FROM turns
             WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND runtime_epoch=?4 AND id=?5",
            params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                owner.runtime_epoch.get() as i64,
                owner.turn_id.as_str()
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(StoreError::Conflict { resource: "turn" })?;
    Ok((
        parse_turn_state(&row.0)?,
        parse_delivery_state(&row.1)?,
        row.2,
    ))
}

fn duplicate(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    state: TurnState,
    delivery_state: DeliveryState,
    revision: i64,
) -> Result<TurnMutation, StoreError> {
    let projection: i64 = transaction.query_row(
        "SELECT projection_revision FROM agent_sessions WHERE id=?1",
        [owner.session_id.as_str()],
        |row| row.get(0),
    )?;
    Ok(TurnMutation {
        state,
        delivery_state,
        revision: EntityRevision::new(revision as u64)
            .map_err(|_| StoreError::Corrupt("turn revision"))?,
        projection_revision: projection_from_i64(projection)?,
        disposition: MutationDisposition::Duplicate,
    })
}
