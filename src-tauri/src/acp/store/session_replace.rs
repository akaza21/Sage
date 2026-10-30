use super::{
    MutationDisposition, SessionMutation, StartSessionCommand, StoreError,
    facts::{hex_digest, projection_from_i64},
    sessions::{ensure_session_capacity, update_chat, valid_digest, validate_chat_revision},
};
use crate::acp::{
    contract::{EntityRevision, SageAgentSessionId},
    lifecycle::SessionState,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplaceSessionCommand {
    pub start: StartSessionCommand,
    pub expected_head: Option<SageAgentSessionId>,
    pub confirm_unknown_delivery: bool,
}

fn request_digest(command: &ReplaceSessionCommand) -> String {
    let start = &command.start;
    hex_digest(serde_json::json!({"project":start.project_id,"chat":start.chat_id,"head":command.expected_head,
        "revision":start.expected_projection_revision,"binding":start.binding_id,"digest":start.binding_digest,
        "version":start.harness_version,"model":start.model_id,"confirmed":command.confirm_unknown_delivery}).to_string().as_bytes())
}

pub(crate) fn lookup(
    connection: &Connection,
    command: &ReplaceSessionCommand,
) -> Result<Option<SessionMutation>, StoreError> {
    let row: Option<(String, String, i64)> = connection.query_row(
        "SELECT request_digest,resulting_session,projection_revision FROM acp_session_transitions WHERE project_id=?1 AND chat_id=?2 AND client_intent_id=?3",
        params![command.start.project_id.as_str(), command.start.chat_id.as_str(), command.start.client_intent_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
    row.map(|(digest, id, revision)| {
        if digest != request_digest(command) {
            return Err(StoreError::Conflict {
                resource: "transitionIntent",
            });
        }
        Ok(SessionMutation {
            session_id: SageAgentSessionId::parse(id)
                .map_err(|_| StoreError::Corrupt("transition session"))?,
            runtime_epoch: None,
            state: SessionState::Created,
            revision: EntityRevision::ZERO,
            projection_revision: projection_from_i64(revision)?,
            disposition: MutationDisposition::Duplicate,
        })
    })
    .transpose()
}

pub(crate) fn preflight(
    connection: &Connection,
    command: &ReplaceSessionCommand,
) -> Result<(), StoreError> {
    let head: Option<String> = connection
        .query_row(
            "SELECT session_id FROM chat_session_heads WHERE project_id=?1 AND chat_id=?2",
            params![
                command.start.project_id.as_str(),
                command.start.chat_id.as_str()
            ],
            |row| row.get(0),
        )
        .optional()?;
    if head.as_deref() != command.expected_head.as_ref().map(|id| id.as_str()) {
        return Err(StoreError::Conflict {
            resource: "chatSessionHead",
        });
    }
    preflight_storage(connection, command)?;
    if !command.confirm_unknown_delivery
        && super::context_handoff::latest_delivery_ambiguous(
            connection,
            command.start.project_id.as_str(),
            command.start.chat_id.as_str(),
        )?
    {
        return Err(StoreError::Conflict {
            resource: "handoffDeliveryUnknown",
        });
    }
    if let Some(head) = head {
        let busy: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE id=?1 AND state IN ('starting','recovering','closing'))
            OR EXISTS(SELECT 1 FROM turns WHERE session_id=?1 AND state IN ('queued','sending','running','waitingModel','waitingTool','waitingApproval','waitingUser','cancelling'))
            OR EXISTS(SELECT 1 FROM server_requests WHERE session_id=?1 AND state IN ('received','asked','allowPending','allowCommitted','declinePending','declineCommitted','responseQueued'))",
            [&head], |row| row.get(0))?;
        if busy {
            return Err(StoreError::Conflict {
                resource: "busySession",
            });
        }
    }
    Ok(())
}

pub(crate) fn replace_session(
    connection: &mut Connection,
    command: ReplaceSessionCommand,
) -> Result<SessionMutation, StoreError> {
    replace_session_at(connection, command, None)
}

pub(crate) fn preflight_storage(
    connection: &Connection,
    command: &ReplaceSessionCommand,
) -> Result<(), StoreError> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM acp_session_transitions WHERE project_id=?1 AND chat_id=?2",
        params![
            command.start.project_id.as_str(),
            command.start.chat_id.as_str()
        ],
        |row| row.get(0),
    )?;
    super::facts::check_add(
        count,
        1,
        crate::acp::limits::LIMITS.durable.sessions_per_chat,
        "chatTransitions",
    )?;
    let bytes: i64 = connection.query_row(
        "SELECT shared_payload_bytes FROM acp_store_usage WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    super::facts::check_add(
        bytes,
        266_240,
        crate::acp::limits::LIMITS
            .durable
            .shared_payload_bytes_global,
        "handoffPayload",
    )
}

pub(crate) fn retain_current(
    connection: &mut Connection,
    command: &ReplaceSessionCommand,
) -> Result<(), StoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if lookup(&transaction, command)?.is_some() {
        return Ok(());
    }
    validate_chat_revision(
        &transaction,
        &command.start.project_id,
        &command.start.chat_id,
        command.start.expected_projection_revision,
    )?;
    preflight(&transaction, command)?;
    let session = command.expected_head.as_ref().ok_or(StoreError::Conflict {
        resource: "transitionHead",
    })?;
    let watermark = crate::acp::context_handoff::history::watermark(
        &transaction,
        command.start.project_id.as_str(),
        command.start.chat_id.as_str(),
    )?;
    record_transition(
        &transaction,
        command,
        session,
        watermark,
        command.start.expected_projection_revision,
    )?;
    super::context_handoff::charge(&transaction, session.as_str(), 4_096)?;
    transaction.commit().map_err(|_| StoreError::CommitUnknown)
}

fn record_transition(
    transaction: &Transaction<'_>,
    command: &ReplaceSessionCommand,
    session: &SageAgentSessionId,
    watermark: i64,
    projection: crate::acp::contract::ProjectionRevision,
) -> Result<(), StoreError> {
    transaction.execute("INSERT INTO acp_session_transitions(project_id,chat_id,client_intent_id,request_digest,expected_head,resulting_session,source_watermark,projection_revision,created_at)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![command.start.project_id.as_str(),command.start.chat_id.as_str(),command.start.client_intent_id.as_str(),request_digest(command),
        command.expected_head.as_ref().map(|id|id.as_str()),session.as_str(),watermark,projection.get() as i64,command.start.now_millis])?;
    Ok(())
}

pub(crate) fn replace_session_at(
    connection: &mut Connection,
    command: ReplaceSessionCommand,
    quiesced_revision: Option<crate::acp::contract::ProjectionRevision>,
) -> Result<SessionMutation, StoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(receipt) = lookup(&transaction, &command)? {
        return Ok(receipt);
    }
    validate_chat_revision(
        &transaction,
        &command.start.project_id,
        &command.start.chat_id,
        quiesced_revision.unwrap_or(command.start.expected_projection_revision),
    )?;
    preflight(&transaction, &command)?;
    ensure_session_capacity(
        &transaction,
        &command.start.project_id,
        &command.start.chat_id,
    )?;
    if !valid_digest(&command.start.binding_digest) {
        return Err(StoreError::Corrupt("target binding digest"));
    }
    retire_old(&transaction, &command)?;
    let (session_id, projection) =
        super::session_insert::insert_session_in(&transaction, &command.start)?;
    let watermark = crate::acp::context_handoff::history::watermark(
        &transaction,
        command.start.project_id.as_str(),
        command.start.chat_id.as_str(),
    )?;
    record_transition(&transaction, &command, &session_id, watermark, projection)?;
    if watermark > 0 {
        transaction.execute("INSERT INTO acp_context_handoffs(id,project_id,chat_id,source_session,target_session,source_watermark,state,projection_revision,created_at,updated_at)
            VALUES(?1,?2,?3,?4,?1,?5,'prepared',?6,?7,?7)", params![session_id.as_str(),command.start.project_id.as_str(),command.start.chat_id.as_str(),
            command.expected_head.as_ref().map(|id|id.as_str()),watermark,projection.get() as i64,command.start.now_millis])?;
    }
    super::context_handoff::charge(&transaction, session_id.as_str(), 4_096)?;
    update_chat(
        &transaction,
        &command.start.project_id,
        &command.start.chat_id,
        projection,
        command.start.now_millis,
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(SessionMutation {
        session_id,
        runtime_epoch: None,
        state: SessionState::Created,
        revision: EntityRevision::ZERO,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}

fn retire_old(
    transaction: &Transaction<'_>,
    command: &ReplaceSessionCommand,
) -> Result<(), StoreError> {
    let Some(old) = &command.expected_head else {
        return Ok(());
    };
    let open: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM session_runtime_epochs WHERE session_id=?1 AND state IN ('starting','running','terminating'))", [old.as_str()], |row| row.get(0))?;
    if open {
        return Err(StoreError::Conflict {
            resource: "runtimeStillLive",
        });
    }
    transaction.execute(
        "DELETE FROM chat_session_heads WHERE session_id=?1",
        [old.as_str()],
    )?;
    transaction.execute("UPDATE agent_sessions SET state='closing',revision=revision+1,updated_at=?2 WHERE id=?1 AND state IN ('created','ready','active')", params![old.as_str(),command.start.now_millis])?;
    transaction.execute("UPDATE agent_sessions SET state='closed',residency='cold',revision=revision+1,projection_revision=(SELECT projection_revision+1 FROM chats WHERE id=agent_sessions.chat_id),updated_at=?2,terminal_at=?2 WHERE id=?1 AND state='closing'", params![old.as_str(),command.start.now_millis])?;
    Ok(())
}
