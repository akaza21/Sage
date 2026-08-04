#![allow(dead_code)] // Removed when the isolated packet receives central wiring.

use rusqlite::params;

use crate::acp::{
    contract::{ProjectionRevision, SageTurnId},
    lifecycle::{DeliveryState, SessionState, TurnState},
    store::{
        RecoveryCandidate, StoreError, load_recovery_candidates, next_projection_revision,
        terminalize_recovery_epoch,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryReport {
    pub recovered_sessions: usize,
    pub terminal_turn: Option<(SageTurnId, TurnState)>,
    pub projection_revision: Option<ProjectionRevision>,
}

pub(crate) fn recover_after_restart(
    connection: &mut rusqlite::Connection,
    now_millis: i64,
) -> Result<RecoveryReport, StoreError> {
    let candidates = load_recovery_candidates(connection)?;
    let Some(candidate) = candidates.into_iter().next() else {
        return Ok(RecoveryReport {
            recovered_sessions: 0,
            terminal_turn: None,
            projection_revision: None,
        });
    };
    recover_candidate(connection, candidate, now_millis)
}

fn recover_candidate(
    connection: &mut rusqlite::Connection,
    candidate: RecoveryCandidate,
    now_millis: i64,
) -> Result<RecoveryReport, StoreError> {
    if candidate.session_state.is_terminal() || candidate.session_state == SessionState::Created {
        return Err(StoreError::Corrupt("live epoch has invalid session state"));
    }
    let transaction = connection.transaction()?;
    let projection = next_projection_revision(
        &transaction,
        candidate.project_id.as_str(),
        candidate.chat_id.as_str(),
    )?;
    let terminalized = terminalize_recovery_epoch(
        &transaction,
        &candidate.project_id,
        &candidate.chat_id,
        &candidate.session_id,
        candidate.runtime_epoch,
        projection,
        now_millis,
    )?;
    let terminal_turn = terminalized.turn;
    ensure_one(
        transaction.execute(
            "UPDATE session_runtime_epochs SET state='failed', terminal_at=?3,
                 exit_kind='restartRecovery'
             WHERE session_id=?1 AND runtime_epoch=?2
               AND state IN ('starting','running','terminating')",
            params![
                candidate.session_id.as_str(),
                candidate.runtime_epoch.get() as i64,
                now_millis
            ],
        )?,
        "runtime epoch",
    )?;
    transaction.execute(
        "DELETE FROM chat_session_heads WHERE session_id=?1",
        [candidate.session_id.as_str()],
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state='failed', residency='cold', revision=revision+1,
                 projection_revision=?2, updated_at=?3, terminal_at=?3 WHERE id=?1",
            params![
                candidate.session_id.as_str(),
                projection.get() as i64,
                now_millis
            ],
        )?,
        "session",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE chats SET projection_revision=?3, updated_at=?4
             WHERE project_id=?1 AND id=?2",
            params![
                candidate.project_id.as_str(),
                candidate.chat_id.as_str(),
                projection.get() as i64,
                now_millis
            ],
        )?,
        "chat",
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(RecoveryReport {
        recovered_sessions: 1,
        terminal_turn,
        projection_revision: Some(projection),
    })
}

fn recovery_turn_state(
    state: TurnState,
    delivery: DeliveryState,
) -> Result<(TurnState, DeliveryState), StoreError> {
    match (state, delivery) {
        (TurnState::Queued, DeliveryState::NotWritten) => Ok((TurnState::Cancelled, delivery)),
        (TurnState::Sending, DeliveryState::NotWritten | DeliveryState::ZeroBytes) => {
            Ok((TurnState::Failed, delivery))
        }
        (TurnState::Sending, DeliveryState::Writing) => {
            Ok((TurnState::DeliveryUnknown, DeliveryState::PossibleBytes))
        }
        (TurnState::Sending, DeliveryState::PossibleBytes | DeliveryState::CompleteBytes) => {
            Ok((TurnState::DeliveryUnknown, delivery))
        }
        (
            TurnState::Running
            | TurnState::WaitingModel
            | TurnState::WaitingTool
            | TurnState::WaitingApproval
            | TurnState::WaitingUser
            | TurnState::Cancelling,
            _,
        ) => Ok((TurnState::Interrupted, delivery)),
        _ => Err(StoreError::Corrupt("invalid recovery turn state")),
    }
}

fn ensure_one(changed: usize, resource: &'static str) -> Result<(), StoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(StoreError::Corrupt(resource))
    }
}

#[cfg(test)]
mod tests;
