// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
use crate::acp::{
    contract::ProjectionRevision,
    lifecycle::{DeliveryState, TurnState},
    store::{StoreError, TurnMutationCommand},
};
use rusqlite::{Transaction, params};

pub(crate) fn observe(
    transaction: &Transaction<'_>,
    command: &TurnMutationCommand,
    state: TurnState,
    delivery: DeliveryState,
    projection: ProjectionRevision,
) -> Result<(), StoreError> {
    let next = match delivery {
        DeliveryState::Writing | DeliveryState::CompleteBytes if state.is_terminal() => "unknown",
        DeliveryState::Writing => "pending",
        DeliveryState::ZeroBytes => "prepared",
        DeliveryState::Correlated => "inline",
        DeliveryState::PossibleBytes => "unknown",
        DeliveryState::NotWritten | DeliveryState::CompleteBytes => return Ok(()),
    };
    transaction.execute(
        "UPDATE acp_context_handoffs SET state=?6,projection_revision=?7,updated_at=?8
        WHERE project_id=?1 AND chat_id=?2 AND target_session=?3 AND runtime_epoch=?4 AND turn_id=?5
        AND state IN ('prepared','pending') AND plan_json IS NOT NULL",
        params![
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.runtime_epoch.get() as i64,
            command.turn_id.as_str(),
            next,
            projection.get() as i64,
            command.now_millis
        ],
    )?;
    Ok(())
}

pub(crate) fn recover_epoch(
    transaction: &Transaction<'_>,
    session: &str,
    epoch: u64,
    projection: ProjectionRevision,
    now: i64,
) -> Result<(), StoreError> {
    transaction.execute("UPDATE acp_context_handoffs SET state='unknown',projection_revision=?3,updated_at=?4 WHERE target_session=?1 AND runtime_epoch=?2 AND state='pending'",
        params![session,epoch as i64,projection.get() as i64,now])?;
    Ok(())
}
