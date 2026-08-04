use rusqlite::{Transaction, params};

use crate::acp::{
    contract::{
        ProjectionRevision, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId,
    },
    lifecycle::{DeliveryState, TurnState},
    limits::LIMITS,
};

use super::{StoreError, facts::append_fact_payload};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalizedCounts {
    pub turns: u64,
    pub items: u64,
    pub activities: u64,
    pub requests: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryTerminalization {
    pub turn: Option<(SageTurnId, TurnState)>,
    pub counts: TerminalizedCounts,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn terminalize_epoch(
    transaction: &Transaction<'_>,
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
    turn_terminal: TurnState,
    fact_kind: &'static str,
    projection: ProjectionRevision,
    now: i64,
) -> Result<TerminalizedCounts, StoreError> {
    if !matches!(
        turn_terminal,
        TurnState::Cancelled | TurnState::Interrupted | TurnState::Failed
    ) {
        return Err(StoreError::InvalidTransition {
            resource: "epochTerminal",
        });
    }
    let turns = count(
        transaction,
        "turns",
        "state IN ('queued','sending','running','waitingModel','waitingTool','waitingApproval','waitingUser','cancelling')",
        session,
        epoch,
    )?;
    let items = count(
        transaction,
        "items",
        "lifecycle IN ('started','streaming')",
        session,
        epoch,
    )?;
    let activities = count(
        transaction,
        "session_activities",
        "lifecycle IN ('observed','active')",
        session,
        epoch,
    )?;
    let requests = count(
        transaction,
        "server_requests",
        "state <> 'resolvedDeclined'",
        session,
        epoch,
    )?;
    if turns > LIMITS.durable.turns_per_session
        || items > LIMITS.durable.items_per_session
        || activities
            > LIMITS
                .durable
                .turns_per_session
                .saturating_mul(LIMITS.native_thread_projection.activity_groups_per_turn)
        || requests > LIMITS.queues.server_requests_per_session
    {
        return Err(StoreError::Corrupt("epoch terminalization limit+1"));
    }
    let turn_state = match turn_terminal {
        TurnState::Cancelled => "cancelled",
        TurnState::Interrupted => "interrupted",
        TurnState::Failed => "failed",
        _ => unreachable!(),
    };
    transaction.execute("UPDATE turns SET state=?3,delivery_state=CASE WHEN delivery_state='writing' THEN 'possibleBytes' ELSE delivery_state END,revision=revision+1,updated_at=?4,terminal_at=?4 WHERE session_id=?1 AND runtime_epoch=?2 AND state IN ('queued','sending','running','waitingModel','waitingTool','waitingApproval','waitingUser','cancelling')",params![session.as_str(),epoch.get() as i64,turn_state,now])?;
    terminalize_descendants(transaction, session, epoch, now)?;
    let payload=serde_json::to_string(&serde_json::json!({"kind":fact_kind,"entityId":session.as_str(),"state":turn_state,"turns":turns,"items":items,"activities":activities,"requests":requests})).map_err(|_|StoreError::Corrupt("terminalization fact"))?;
    append_fact_payload(
        transaction,
        project.as_str(),
        chat.as_str(),
        session.as_str(),
        None,
        fact_kind,
        &payload,
        projection,
        now,
    )?;
    Ok(TerminalizedCounts {
        turns,
        items,
        activities,
        requests,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn terminalize_recovery_epoch(
    transaction: &Transaction<'_>,
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
    projection: ProjectionRevision,
    now: i64,
) -> Result<RecoveryTerminalization, StoreError> {
    let mut statement = transaction.prepare(
        "SELECT id,state,delivery_state FROM turns WHERE session_id=?1 AND runtime_epoch=?2
         AND state IN ('queued','sending','running','waitingModel','waitingTool',
                       'waitingApproval','waitingUser','cancelling') LIMIT 2",
    )?;
    let rows = statement.query_map(params![session.as_str(), epoch.get() as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut turns = Vec::with_capacity(2);
    for row in rows {
        turns.push(row?);
    }
    if turns.len() > 1 {
        return Err(StoreError::Corrupt("multiple live turns in runtime epoch"));
    }
    let turn = turns
        .first()
        .map(|row| {
            let terminal = recovery_turn_state(&row.1, &row.2)?;
            Ok::<(SageTurnId, TurnState, DeliveryState), StoreError>((
                SageTurnId::parse(row.0.clone()).map_err(|_| StoreError::Corrupt("turn id"))?,
                terminal.0,
                terminal.1,
            ))
        })
        .transpose()?;
    let counts = precheck_descendants(transaction, session, epoch, turns.len() as u64)?;
    if let Some((turn_id, state, delivery)) = &turn {
        transaction.execute(
            "UPDATE turns SET state=?2,delivery_state=?3,revision=revision+1,
             updated_at=?4,terminal_at=?4 WHERE id=?1",
            params![
                turn_id.as_str(),
                turn_state(*state),
                delivery_state(*delivery),
                now
            ],
        )?;
    }
    terminalize_descendants(transaction, session, epoch, now)?;
    let payload=serde_json::to_string(&serde_json::json!({"kind":"restartRecovery","entityId":session.as_str(),"state":"sessionFailed","turns":counts.turns,"items":counts.items,"activities":counts.activities,"requests":counts.requests})).map_err(|_|StoreError::Corrupt("recovery fact"))?;
    append_fact_payload(
        transaction,
        project.as_str(),
        chat.as_str(),
        session.as_str(),
        turn.as_ref().map(|value| value.0.as_str()),
        "restartRecovery",
        &payload,
        projection,
        now,
    )?;
    Ok(RecoveryTerminalization {
        turn: turn.map(|(id, state, _)| (id, state)),
        counts,
    })
}

fn precheck_descendants(
    transaction: &Transaction<'_>,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
    turns: u64,
) -> Result<TerminalizedCounts, StoreError> {
    let items = count(
        transaction,
        "items",
        "lifecycle IN ('started','streaming')",
        session,
        epoch,
    )?;
    let activities = count(
        transaction,
        "session_activities",
        "lifecycle IN ('observed','active')",
        session,
        epoch,
    )?;
    let requests = count(
        transaction,
        "server_requests",
        "state <> 'resolvedDeclined'",
        session,
        epoch,
    )?;
    if turns > 1
        || items > LIMITS.durable.items_per_session
        || activities
            > LIMITS
                .durable
                .turns_per_session
                .saturating_mul(LIMITS.native_thread_projection.activity_groups_per_turn)
        || requests > LIMITS.queues.server_requests_per_session
    {
        return Err(StoreError::Corrupt("epoch terminalization limit+1"));
    }
    Ok(TerminalizedCounts {
        turns,
        items,
        activities,
        requests,
    })
}

fn terminalize_descendants(
    transaction: &Transaction<'_>,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
    now: i64,
) -> Result<(), StoreError> {
    transaction.execute("UPDATE items SET lifecycle='cancelled',tool_state=CASE WHEN tool_state IN ('pending','inProgress','waitingForApproval') THEN 'cancelled' ELSE tool_state END,revision=revision+1,updated_at=?3,terminal_at=?3 WHERE session_id=?1 AND runtime_epoch=?2 AND lifecycle IN ('started','streaming')",params![session.as_str(),epoch.get() as i64,now])?;
    transaction.execute("UPDATE session_activities SET lifecycle='cancelled',revision=revision+1,updated_at=?3,terminal_at=?3 WHERE session_id=?1 AND runtime_epoch=?2 AND lifecycle IN ('observed','active')",params![session.as_str(),epoch.get() as i64,now])?;
    for (old, next) in [
        ("received", "declinePending"),
        ("declinePending", "declineCommitted"),
        ("declineCommitted", "responseQueued"),
        ("responseQueued", "resolvedDeclined"),
    ] {
        transaction.execute("UPDATE server_requests SET state=?3,revision=revision+1,updated_at=?4,resolved_at=CASE WHEN ?3='resolvedDeclined' THEN ?4 ELSE resolved_at END WHERE session_id=?1 AND runtime_epoch=?2 AND state=?5",params![session.as_str(),epoch.get() as i64,next,now,old])?;
    }
    Ok(())
}

fn recovery_turn_state(
    state: &str,
    delivery: &str,
) -> Result<(TurnState, DeliveryState), StoreError> {
    use DeliveryState::*;
    use TurnState::*;
    let delivery = match delivery {
        "notWritten" => NotWritten,
        "writing" => Writing,
        "zeroBytes" => ZeroBytes,
        "possibleBytes" => PossibleBytes,
        "completeBytes" => CompleteBytes,
        "correlated" => Correlated,
        _ => return Err(StoreError::Corrupt("delivery state")),
    };
    match (state, delivery) {
        ("queued", NotWritten) => Ok((Cancelled, NotWritten)),
        ("sending", NotWritten | ZeroBytes) => Ok((Failed, delivery)),
        ("sending", Writing) => Ok((DeliveryUnknown, PossibleBytes)),
        ("sending", PossibleBytes | CompleteBytes) => Ok((DeliveryUnknown, delivery)),
        (
            "running" | "waitingModel" | "waitingTool" | "waitingApproval" | "waitingUser"
            | "cancelling",
            value,
        ) => Ok((Interrupted, value)),
        _ => Err(StoreError::Corrupt("invalid recovery turn state")),
    }
}

fn turn_state(value: TurnState) -> &'static str {
    match value {
        TurnState::Cancelled => "cancelled",
        TurnState::Failed => "failed",
        TurnState::Interrupted => "interrupted",
        TurnState::DeliveryUnknown => "deliveryUnknown",
        _ => "invalid",
    }
}
fn delivery_state(value: DeliveryState) -> &'static str {
    match value {
        DeliveryState::NotWritten => "notWritten",
        DeliveryState::Writing => "writing",
        DeliveryState::ZeroBytes => "zeroBytes",
        DeliveryState::PossibleBytes => "possibleBytes",
        DeliveryState::CompleteBytes => "completeBytes",
        DeliveryState::Correlated => "correlated",
    }
}

fn count(
    transaction: &Transaction<'_>,
    table: &str,
    predicate: &str,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
) -> Result<u64, StoreError> {
    let sql = format!(
        "SELECT COUNT(*) FROM {table} WHERE session_id=?1 AND runtime_epoch=?2 AND {predicate}"
    );
    let value: i64 =
        transaction.query_row(&sql, params![session.as_str(), epoch.get() as i64], |row| {
            row.get(0)
        })?;
    u64::try_from(value).map_err(|_| StoreError::Corrupt("terminalization count"))
}
