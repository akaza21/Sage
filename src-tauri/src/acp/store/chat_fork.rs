//! Durable native-fork lineage. Rust alone decides whether a Chat may be
//! forked, then installs the fork's resumable session head and the immutable
//! `chat_lineage` row in the same transaction that creates the fork Chat.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId},
    intent::{CapabilitySnapshotV1, ExternalId},
};

use super::{
    StoreError,
    facts::{append_fact_payload, next_projection_revision},
    session_head::{HeadSession, install_head_session},
    sessions::{ensure_one, ensure_session_capacity, update_chat},
};

/// A validated loaded-history target of the Chat's live head session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForkSource {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
    pub target: Option<sage_acp_protocol::session_control::ConversationTarget>,
}

/// The offer selects arbitrary prompt targets or a legacy latest-turn fork.
/// Both require an idle head; temporary side chats are never fork sources.
pub(crate) fn fork_source(
    connection: &Connection,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    turn_id: &SageTurnId,
) -> Result<ForkSource, StoreError> {
    if crate::side_chat_storage::is_temporary(connection, chat_id.as_str())
        .map_err(|_| StoreError::Corrupt("temporary Chat metadata"))?
    {
        return Err(StoreError::Conflict {
            resource: "forkSource",
        });
    }
    let head: Option<(String, String, String, String, i64)> = connection
        .query_row(
            "SELECT s.id,s.state,s.residency,s.capabilities_json,e.runtime_epoch
             FROM chat_session_heads h
             JOIN agent_sessions s ON s.id=h.session_id AND s.project_id=h.project_id
               AND s.chat_id=h.chat_id
             JOIN session_runtime_epochs e ON e.session_id=s.id AND e.state='running'
             WHERE h.project_id=?1 AND h.chat_id=?2",
            params![project_id.as_str(), chat_id.as_str()],
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
    let Some((session, state, residency, capabilities, epoch)) = head else {
        return Err(StoreError::Conflict {
            resource: "forkRuntime",
        });
    };
    let rewind = serde_json::from_str::<CapabilitySnapshotV1>(&capabilities)
        .map(|snapshot| snapshot.rewind)
        .unwrap_or_default();
    let admits_fork = rewind.fork_at;
    if !admits_fork || residency != "hot" || !matches!(state.as_str(), "ready" | "active") {
        return Err(StoreError::Conflict {
            resource: "forkCapability",
        });
    }
    super::turn_undo::intent::ensure_settled(connection, project_id.as_str(), chat_id.as_str())?;
    if !super::rewind::has_identity(connection, &session, turn_id.as_str())? {
        return Err(StoreError::RewindUnavailable {
            reason: "Older messages can't be rewound.",
        });
    }
    let target = if rewind.targets == sage_acp_protocol::intent::RewindTargets::LatestTurn {
        require_latest_completed(connection, &session, turn_id)?;
        None
    } else {
        Some(super::rewind::target(
            connection,
            project_id.as_str(),
            chat_id.as_str(),
            &session,
            turn_id.as_str(),
        )?)
    };
    Ok(ForkSource {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: SageAgentSessionId::parse(session)
            .map_err(|_| StoreError::Corrupt("fork session id"))?,
        runtime_epoch: u64::try_from(epoch)
            .ok()
            .and_then(|value| RuntimeEpoch::new(value).ok())
            .ok_or(StoreError::Corrupt("fork runtime epoch"))?,
        turn_id: turn_id.clone(),
        target,
    })
}

/// The fork is cut at the native thread's latest reply, so the Turn must be
/// the session's latest visible one and no undone Turn may still wait in
/// that thread for its rewind.
fn require_latest_completed(
    connection: &Connection,
    session: &str,
    turn_id: &SageTurnId,
) -> Result<(), StoreError> {
    if super::turn_undo::pending_thread_rewind(connection, session)?.is_some() {
        return Err(StoreError::Conflict {
            resource: "forkTurn",
        });
    }
    let latest: Option<(String, String)> = connection
        .query_row(
            concat!(
                "SELECT t.id,t.state FROM turns t WHERE t.session_id=?1 AND ",
                super::turn_undo::visible_turn!("t.session_id", "t.id"),
                " ORDER BY t.ordinal DESC LIMIT 1"
            ),
            [session],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match latest {
        Some((id, state)) if id == turn_id.as_str() && state == "completed" => Ok(()),
        _ => Err(StoreError::Conflict {
            resource: "forkTurn",
        }),
    }
}

/// Installs the fork's head session (cold, `ready`, bound to the native fork
/// so the first send resumes it) and the immutable lineage row. The caller
/// has just created `child_chat_id` in the same transaction. The source is
/// re-validated here: a Turn started while the native fork ran refuses it.
pub(crate) fn install_fork(
    transaction: &Transaction<'_>,
    source: &ForkSource,
    child_chat_id: &SageChatId,
    external_session_id: &ExternalId,
    now_millis: i64,
) -> Result<SageAgentSessionId, StoreError> {
    let current = fork_source(
        transaction,
        &source.project_id,
        &source.chat_id,
        &source.turn_id,
    )?;
    if &current != source {
        return Err(StoreError::Conflict {
            resource: "forkSource",
        });
    }
    let (binding_id, binding_digest, harness_version, model_id, capabilities): (
        String,
        String,
        Option<String>,
        Option<String>,
        String,
    ) = transaction.query_row(
        "SELECT binding_id,binding_digest,harness_version,model_id,capabilities_json
         FROM agent_sessions WHERE id=?1",
        [source.session_id.as_str()],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    ensure_session_capacity(transaction, &source.project_id, child_chat_id)?;
    let project = source.project_id.as_str();
    let child = child_chat_id.as_str();
    let projection = next_projection_revision(transaction, project, child)?;
    let session_id = install_head_session(
        transaction,
        HeadSession {
            project,
            chat: child,
            ordinal: 0,
            binding_id: &binding_id,
            binding_digest: &binding_digest,
            harness_version: harness_version.as_deref(),
            model_id: model_id.as_deref(),
            external_session_id: Some(external_session_id.as_str()),
            resumes_session_id: None,
            capabilities_json: &capabilities,
            state: "ready",
        },
        projection,
        now_millis,
    )?;
    // The fork continues the parent's session, so it keeps the parent's
    // confirmed config choices.
    crate::acp::session_config::choices::copy(transaction, &source.session_id, &session_id)?;
    let payload = serde_json::json!({
        "kind": "sessionForked",
        "entityId": session_id.as_str(),
        "state": "committed",
        "parentChatId": source.chat_id.as_str(),
        "sourceTurnId": source.turn_id.as_str(),
        "beforePrompt": source.target.is_some(),
        "sourceSequence": transaction.query_row("SELECT COALESCE(MAX(sequence),0) FROM session_facts WHERE session_id=?1",[source.session_id.as_str()],|r|r.get::<_,i64>(0))?,
    })
    .to_string();
    append_fact_payload(
        transaction,
        project,
        child,
        session_id.as_str(),
        None,
        "sessionForked",
        &payload,
        projection,
        now_millis,
    )?;
    update_chat(
        transaction,
        &source.project_id,
        child_chat_id,
        projection,
        now_millis,
    )?;
    ensure_one(
        transaction.execute(
            "INSERT INTO chat_lineage(child_project_id,child_chat_id,parent_project_id,
               parent_chat_id,source_session_id,source_turn_id,relation,
               external_operation_id,created_at)
             VALUES(?1,?2,?1,?3,?4,?5,'nativeFork',NULL,?6)",
            params![
                project,
                child,
                source.chat_id.as_str(),
                source.session_id.as_str(),
                source.turn_id.as_str(),
                now_millis
            ],
        )?,
        "chat lineage",
    )?;
    Ok(session_id)
}

/// The parent of a forked Chat and the parent Turn the fork carries history
/// through, for the inherited history and the "Continued from chat" link.
pub(crate) fn forked_from(
    connection: &Connection,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
) -> Result<Option<(SageChatId, SageTurnId)>, StoreError> {
    connection
        .query_row(
            "SELECT parent_chat_id,source_turn_id FROM chat_lineage
             WHERE child_project_id=?1 AND child_chat_id=?2 AND relation='nativeFork'",
            params![project_id.as_str(), chat_id.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .map(|(parent, turn)| {
            Ok((
                SageChatId::parse(parent)
                    .map_err(|_| StoreError::Corrupt("fork parent chat id"))?,
                SageTurnId::parse(turn).map_err(|_| StoreError::Corrupt("fork source turn id"))?,
            ))
        })
        .transpose()
}

pub(crate) struct ForkHistory {
    pub session: String,
    pub session_ordinal: i64,
    pub through_ordinal: i64,
    pub sequence: i64,
}

pub(crate) fn history_boundary(
    connection: &Connection,
    project: &SageProjectId,
    parent: &SageChatId,
    child: &SageChatId,
) -> Result<ForkHistory, StoreError> {
    connection.query_row("SELECT s.id,s.ordinal,t.ordinal-COALESCE(json_extract(f.payload_json,'$.beforePrompt'),0),
        COALESCE(json_extract(f.payload_json,'$.sourceSequence'),(SELECT COALESCE(MAX(sequence),0) FROM session_facts old
          WHERE old.session_id=s.id AND old.created_at<=l.created_at))
        FROM chat_lineage l JOIN agent_sessions s ON s.id=l.source_session_id
        JOIN turns t ON t.id=l.source_turn_id
        LEFT JOIN session_facts f ON f.project_id=l.child_project_id AND f.chat_id=l.child_chat_id AND f.kind='sessionForked'
        WHERE l.child_project_id=?1 AND l.parent_project_id=?1 AND l.parent_chat_id=?2 AND l.child_chat_id=?3 AND l.relation='nativeFork'
        LIMIT 1",params![project.as_str(),parent.as_str(),child.as_str()],|r|Ok(ForkHistory{session:r.get(0)?,session_ordinal:r.get(1)?,through_ordinal:r.get(2)?,sequence:r.get(3)?})).map_err(Into::into)
}
