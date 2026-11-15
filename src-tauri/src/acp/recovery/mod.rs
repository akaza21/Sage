use rusqlite::{OptionalExtension, params};

use crate::acp::{
    runtime::terminal_detail::sage_closed,
    store::{
        RecoverRuntimeCommand, RecoveryCandidate, StoreError, load_recovery_candidates,
        next_projection_revision, recordable_terminal_reason, terminalize_recovery_epoch,
    },
};
#[cfg(test)]
use sage_acp_protocol::lifecycle::DeliveryState;
use sage_acp_protocol::{
    contract::{ProjectionRevision, SageTurnId},
    lifecycle::{SessionState, TurnState, TurnTerminalOutcome},
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
    // Every bounded live tree from the previous process is recovered; each
    // candidate commits in its own transaction, oldest first.
    let mut report = RecoveryReport {
        recovered_sessions: 0,
        terminal_turn: None,
        projection_revision: None,
    };
    let closed = sage_closed();
    for candidate in load_recovery_candidates(connection)? {
        let recovered = recover_candidate(connection, candidate, now_millis, &closed)?;
        report.recovered_sessions += recovered.recovered_sessions;
        report.terminal_turn = recovered.terminal_turn.or(report.terminal_turn);
        report.projection_revision = recovered.projection_revision;
    }
    let (parked, revision) =
        crate::acp::store::turn_undo::recovery::recover_parked(connection, now_millis)?;
    report.recovered_sessions += parked;
    report.projection_revision = revision.or(report.projection_revision);
    Ok(report)
}

pub(crate) fn recover_runtime_after_zero_process(
    connection: &mut rusqlite::Connection,
    command: RecoverRuntimeCommand,
) -> Result<RecoveryReport, StoreError> {
    // Other bounded live trees may coexist; only this exact owner's epoch is
    // recovered, and a missing candidate replays an exact postcondition.
    let candidates = load_recovery_candidates(connection)?;
    let report = match candidates
        .into_iter()
        .find(|candidate| exact_candidate(candidate, &command))
    {
        Some(candidate) => recover_candidate(
            connection,
            candidate,
            command.now_millis,
            &recordable_terminal_reason(command.terminal_reason.clone()),
        ),
        None => replay_recovered_runtime(connection, &command),
    }?;
    Ok(report)
}

fn exact_candidate(candidate: &RecoveryCandidate, command: &RecoverRuntimeCommand) -> bool {
    candidate.project_id == command.project_id
        && candidate.chat_id == command.chat_id
        && candidate.session_id == command.session_id
        && candidate.runtime_epoch == command.runtime_epoch
}

/// Replays only an exact recovery postcondition. Returning the persisted
/// projection revision makes commit-unknown and publication failures safely
/// retryable without treating an arbitrary missing candidate as success.
fn replay_recovered_runtime(
    connection: &rusqlite::Connection,
    command: &RecoverRuntimeCommand,
) -> Result<RecoveryReport, StoreError> {
    let row = connection
        .query_row(
            "SELECT s.state,s.residency,s.projection_revision,e.state,e.exit_kind,
                    EXISTS(SELECT 1 FROM chat_session_heads h WHERE h.session_id=s.id),
                    EXISTS(SELECT 1 FROM turns t WHERE t.session_id=s.id
                      AND t.runtime_epoch=e.runtime_epoch AND t.state IN
                      ('queued','sending','running','waitingModel','waitingTool',
                       'waitingApproval','waitingUser','cancelling'))
             FROM agent_sessions s JOIN session_runtime_epochs e ON e.session_id=s.id
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3 AND e.runtime_epoch=?4",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, bool>(5)?,
                    row.get::<_, bool>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((session, residency, revision, epoch, exit, has_head, has_live_turn)) = row else {
        return Err(StoreError::Conflict {
            resource: "runtimeRecovery",
        });
    };
    let parked = session == "ready"
        && epoch == "exited"
        && has_head
        && crate::acp::store::pending_thread_rewind(connection, command.session_id.as_str())?
            .is_some();
    let retired = session == "failed" && epoch == "failed" && !has_head;
    if !(parked || retired)
        || residency != "cold"
        || exit.as_deref() != Some("restartRecovery")
        || has_live_turn
    {
        return Err(StoreError::Corrupt("runtime recovery postcondition"));
    }
    let projection_revision = u64::try_from(revision)
        .ok()
        .and_then(|value| ProjectionRevision::new(value).ok())
        .ok_or(StoreError::Corrupt("runtime recovery projection"))?;
    Ok(RecoveryReport {
        recovered_sessions: 0,
        terminal_turn: None,
        projection_revision: Some(projection_revision),
    })
}

fn recover_candidate(
    connection: &mut rusqlite::Connection,
    candidate: RecoveryCandidate,
    now_millis: i64,
    reason: &TurnTerminalOutcome,
) -> Result<RecoveryReport, StoreError> {
    if candidate.session_state.is_terminal() || candidate.session_state == SessionState::Created {
        return Err(StoreError::Corrupt("live epoch has invalid session state"));
    }
    let transaction = connection.transaction()?;
    let undo_rolled_back =
        crate::acp::store::turn_undo::recovery::settle_live(&transaction, &candidate, now_millis)?;
    // A pending cut proves the undo committed, not that a later prompt was
    // never delivered. Include terminal deliveryUnknown turns too: their
    // native effects remain uncertain even though they are no longer live.
    let park = !undo_rolled_back
        && matches!(
            candidate.session_state,
            SessionState::Ready | SessionState::Active
        )
        && match crate::acp::store::pending_thread_rewind(
            &transaction,
            candidate.session_id.as_str(),
        )? {
            Some(rewind) => transaction.query_row(
                "SELECT NOT EXISTS(SELECT 1 FROM turns t
                 JOIN turns undone ON undone.session_id=t.session_id AND undone.id=?3
                 WHERE t.session_id=?1 AND t.runtime_epoch=?2 AND t.ordinal>undone.ordinal
                   AND t.delivery_state NOT IN ('notWritten','zeroBytes'))",
                params![
                    candidate.session_id.as_str(),
                    candidate.runtime_epoch.get() as i64,
                    rewind.turn_id.as_str()
                ],
                |row| row.get::<_, bool>(0),
            )?,
            None => false,
        };
    let projection = next_projection_revision(
        &transaction,
        candidate.project_id.as_str(),
        candidate.chat_id.as_str(),
    )?;
    let terminal_turn = if park {
        crate::acp::store::terminalize_epoch(
            &transaction,
            &candidate.project_id,
            &candidate.chat_id,
            &candidate.session_id,
            candidate.runtime_epoch,
            TurnState::Cancelled,
            "undoRecovery",
            projection,
            now_millis,
        )?;
        None
    } else {
        terminalize_recovery_epoch(
            &transaction,
            &candidate.project_id,
            &candidate.chat_id,
            &candidate.session_id,
            candidate.runtime_epoch,
            reason,
            projection,
            now_millis,
        )?
        .turn
    };
    ensure_one(
        transaction.execute(
            "UPDATE session_runtime_epochs SET state=?4, terminal_at=?3,
                 exit_kind='restartRecovery'
             WHERE session_id=?1 AND runtime_epoch=?2
               AND state IN ('starting','running','terminating')",
            params![
                candidate.session_id.as_str(),
                candidate.runtime_epoch.get() as i64,
                now_millis,
                if park { "exited" } else { "failed" }
            ],
        )?,
        "runtime epoch",
    )?;
    if !park {
        transaction.execute(
            "DELETE FROM chat_session_heads WHERE session_id=?1",
            [candidate.session_id.as_str()],
        )?;
    }
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state=?4, residency='cold', revision=revision+1,
                 projection_revision=?2, updated_at=?3, terminal_at=?5 WHERE id=?1",
            params![
                candidate.session_id.as_str(),
                projection.get() as i64,
                now_millis,
                if park { "ready" } else { "failed" },
                if park { None } else { Some(now_millis) }
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

#[cfg(test)]
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
        ) => Ok((TurnState::Failed, delivery)),
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
