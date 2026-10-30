use rusqlite::{Transaction, params, params_from_iter, types::Value};

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
    let scope = DescendantScope {
        session,
        epoch,
        turn: None,
    };
    let turns = count(transaction, "turns", OPEN_TURNS, scope)?;
    let items = count(transaction, "items", scope.open_items(), scope)?;
    let activities = count(transaction, "session_activities", OPEN_ACTIVITIES, scope)?;
    let requests = count(transaction, "server_requests", OPEN_REQUESTS, scope)?;
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
    crate::turn_workspace::settle_terminal(transaction, session.as_str())?;
    terminalize_descendants(transaction, session, epoch, now)?;
    crate::acp::context_handoff::delivery::recover_epoch(
        transaction,
        session.as_str(),
        epoch.get(),
        projection,
        now,
    )?;
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
    crate::acp::context_handoff::delivery::recover_epoch(
        transaction,
        session.as_str(),
        epoch.get(),
        projection,
        now,
    )?;
    crate::turn_workspace::settle_terminal(transaction, session.as_str())?;
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
    let scope = DescendantScope {
        session,
        epoch,
        turn: None,
    };
    let items = count(transaction, "items", scope.open_items(), scope)?;
    let activities = count(transaction, "session_activities", OPEN_ACTIVITIES, scope)?;
    let requests = count(transaction, "server_requests", OPEN_REQUESTS, scope)?;
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

#[derive(Clone, Copy)]
struct DescendantScope<'a> {
    session: &'a SageAgentSessionId,
    epoch: RuntimeEpoch,
    turn: Option<&'a SageTurnId>,
}

impl DescendantScope<'_> {
    fn clause(&self, turn_index: u8) -> String {
        match self.turn {
            Some(_) => format!(" AND turn_id=?{turn_index}"),
            None => String::new(),
        }
    }

    fn open_items(&self) -> &'static str {
        match self.turn {
            Some(_) => OPEN_TURN_ITEMS,
            None => OPEN_ITEMS,
        }
    }

    fn values(&self, now: Option<i64>) -> Vec<Value> {
        let mut values = vec![
            Value::Text(self.session.as_str().to_owned()),
            Value::Integer(self.epoch.get() as i64),
        ];
        values.extend(now.map(Value::Integer));
        values.extend(self.turn.map(|turn| Value::Text(turn.as_str().to_owned())));
        values
    }
}

const OPEN_TURNS: &str = "state IN ('queued','sending','running','waitingModel','waitingTool','waitingApproval','waitingUser','cancelling')";
const OPEN_ITEMS: &str = "lifecycle IN ('started','streaming')";
const OPEN_TURN_ITEMS: &str = "lifecycle IN ('started','streaming') AND kind<>'permissionRequest'";
const OPEN_ACTIVITIES: &str = "lifecycle IN ('observed','active')";
const OPEN_REQUESTS: &str = "state NOT IN ('resolvedAllowed','resolvedDeclined','cancelled')";

fn terminalize_open_descendants(
    transaction: &Transaction<'_>,
    scope: DescendantScope<'_>,
    now: i64,
) -> Result<(), StoreError> {
    let turn_clause = scope.clause(4);
    let open_items = scope.open_items();
    transaction.execute(
        &format!(
            "UPDATE items SET lifecycle=CASE WHEN kind='permissionRequest' THEN 'failed' ELSE 'cancelled' END,tool_state=CASE WHEN kind='permissionRequest' THEN 'failed' WHEN tool_state IN ('pending','inProgress','waitingForApproval') THEN 'cancelled' ELSE tool_state END,revision=revision+1,updated_at=?3,terminal_at=?3 WHERE session_id=?1 AND runtime_epoch=?2 AND {open_items}{turn_clause}"
        ),
        params_from_iter(scope.values(Some(now))),
    )?;
    transaction.execute(
        &format!(
            "UPDATE session_activities SET lifecycle='cancelled',revision=revision+1,updated_at=?3,terminal_at=?3 WHERE session_id=?1 AND runtime_epoch=?2 AND {OPEN_ACTIVITIES}{turn_clause}"
        ),
        params_from_iter(scope.values(Some(now))),
    )?;
    Ok(())
}

pub(super) fn terminalize_turn_descendants(
    transaction: &Transaction<'_>,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
    turn: &SageTurnId,
    now: i64,
) -> Result<(), StoreError> {
    let scope = DescendantScope {
        session,
        epoch,
        turn: Some(turn),
    };
    if count(transaction, "items", scope.open_items(), scope)? > LIMITS.wire.items_per_turn
        || count(transaction, "session_activities", OPEN_ACTIVITIES, scope)?
            > LIMITS.native_thread_projection.activity_groups_per_turn
    {
        return Err(StoreError::Corrupt("turn terminalization limit+1"));
    }
    terminalize_open_descendants(transaction, scope, now)
}

fn terminalize_descendants(
    transaction: &Transaction<'_>,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
    now: i64,
) -> Result<(), StoreError> {
    terminalize_open_descendants(
        transaction,
        DescendantScope {
            session,
            epoch,
            turn: None,
        },
        now,
    )?;
    for (old, next) in [
        ("received", "declinePending"),
        ("asked", "declinePending"),
        ("allowPending", "declinePending"),
        ("allowCommitted", "declinePending"),
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
        ) => Ok((Failed, value)),
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
    scope: DescendantScope<'_>,
) -> Result<u64, StoreError> {
    let turn_clause = scope.clause(3);
    let sql = format!(
        "SELECT COUNT(*) FROM {table} WHERE session_id=?1 AND runtime_epoch=?2 AND {predicate}{turn_clause}"
    );
    let value: i64 =
        transaction.query_row(&sql, params_from_iter(scope.values(None)), |row| row.get(0))?;
    u64::try_from(value).map_err(|_| StoreError::Corrupt("terminalization count"))
}
