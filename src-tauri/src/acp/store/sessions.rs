use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::{
    contract::{
        ClientIntentId, EntityRevision, ProjectionRevision, RuntimeEpoch, SafeText,
        SageAgentSessionId, SageChatId, SageProjectId,
    },
    intent::{BindingId, ModelId},
    lifecycle::SessionState,
    limits::LIMITS,
};

pub(super) const PENDING_CAPABILITIES_JSON: &str = r#"{"schemaVersion":1,"protocolVersion":"pending","newSession":false,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":false,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}"#;

use super::{
    MutationDisposition, StoreError,
    facts::{append_fact_payload, check_add, entity_revision_from_i64, next_projection_revision},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartSessionCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub binding_id: BindingId,
    pub binding_digest: String,
    pub harness_version: Option<SafeText<128>>,
    pub model_id: Option<ModelId>,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivateSessionCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub artifact_digest: String,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CloseSessionCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionMutation {
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: Option<RuntimeEpoch>,
    pub state: SessionState,
    pub revision: EntityRevision,
    pub projection_revision: ProjectionRevision,
    pub disposition: MutationDisposition,
}

pub(crate) fn start_session(
    connection: &mut rusqlite::Connection,
    command: StartSessionCommand,
) -> Result<SessionMutation, StoreError> {
    validate_start(&command)?;
    let transaction = connection.transaction()?;
    if let Some(duplicate) = super::session_duplicates::start_duplicate(&transaction, &command)? {
        return Ok(duplicate);
    }
    validate_chat_revision(
        &transaction,
        &command.project_id,
        &command.chat_id,
        command.expected_projection_revision,
    )?;
    ensure_session_capacity(&transaction, &command.project_id, &command.chat_id)?;
    let occupied: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM chat_session_heads WHERE project_id=?1 AND chat_id=?2)",
        params![command.project_id.as_str(), command.chat_id.as_str()],
        |row| row.get(0),
    )?;
    if occupied {
        return Err(StoreError::Conflict {
            resource: "chatSessionHead",
        });
    }
    let ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal),-1)+1 FROM agent_sessions WHERE chat_id=?1",
        [command.chat_id.as_str()],
        |row| row.get(0),
    )?;
    if ordinal < 0 {
        return Err(StoreError::CounterExhausted);
    }
    let session_id = SageAgentSessionId::new();
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    transaction.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,harness_version,model_id,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at,terminal_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,NULL,?9,'created','cold',0,0,?10,?11,?11,NULL)",params![session_id.as_str(),command.project_id.as_str(),command.chat_id.as_str(),ordinal,command.binding_id.as_str(),command.binding_digest,command.harness_version.as_ref().map(|v|v.as_str()),command.model_id.as_ref().map(|v|v.as_str()),PENDING_CAPABILITIES_JSON,projection.get() as i64,command.now_millis])?;
    transaction.execute("INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,item_bytes,fact_count,fact_bytes,payload_bytes,revision) VALUES(?1,?2,?3,0,0,0,0,0,0,0)",params![command.project_id.as_str(),command.chat_id.as_str(),session_id.as_str()])?;
    increment_session_usage(&transaction, command.project_id.as_str())?;
    transaction.execute("INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES(?1,?2,?3,?4,?5)",params![command.project_id.as_str(),command.chat_id.as_str(),session_id.as_str(),projection.get() as i64,command.now_millis])?;
    append_intent_fact(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &session_id,
        None,
        "sessionStartIntent",
        &command.client_intent_id,
        projection,
        command.now_millis,
    )?;
    update_chat(
        &transaction,
        &command.project_id,
        &command.chat_id,
        projection,
        command.now_millis,
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

pub(crate) fn activate_session(
    connection: &mut rusqlite::Connection,
    command: ActivateSessionCommand,
) -> Result<SessionMutation, StoreError> {
    if !valid_digest(&command.artifact_digest) {
        return Err(StoreError::Corrupt("artifact digest"));
    }
    let transaction = connection.transaction()?;
    if let Some(value) = super::session_duplicates::activation_duplicate(&transaction, &command)? {
        return Ok(value);
    }
    validate_chat_revision(
        &transaction,
        &command.project_id,
        &command.chat_id,
        command.expected_projection_revision,
    )?;
    let (state,revision):(String,i64)=transaction.query_row("SELECT state,revision FROM agent_sessions WHERE project_id=?1 AND chat_id=?2 AND id=?3",params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str()],|row|Ok((row.get(0)?,row.get(1)?))).optional()?.ok_or(StoreError::Conflict{resource:"session"})?;
    let target = match state.as_str() {
        "created" => "starting",
        "ready" => "recovering",
        _ => {
            return Err(StoreError::InvalidTransition {
                resource: "sessionActivation",
            });
        }
    };
    let head:bool=transaction.query_row("SELECT EXISTS(SELECT 1 FROM chat_session_heads WHERE project_id=?1 AND chat_id=?2 AND session_id=?3)",params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str()],|row|row.get(0))?;
    let open:i64=transaction.query_row("SELECT COUNT(*) FROM session_runtime_epochs WHERE state IN ('starting','running','terminating')",[],|row|row.get(0))?;
    let maximum: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(runtime_epoch),0) FROM session_runtime_epochs WHERE session_id=?1",
        [command.session_id.as_str()],
        |row| row.get(0),
    )?;
    if !head || open != 0 || maximum.checked_add(1) != Some(command.runtime_epoch.get() as i64) {
        return Err(StoreError::Conflict {
            resource: "runtimeEpoch",
        });
    }
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    transaction.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at,terminal_at,exit_kind) VALUES(?1,?2,?3,?4,'starting',?5,?6,NULL,NULL)",params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str(),command.runtime_epoch.get() as i64,command.artifact_digest,command.now_millis])?;
    let next = entity_revision_from_i64(revision)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    ensure_one(transaction.execute("UPDATE agent_sessions SET state=?2,residency='hot',revision=?3,projection_revision=?4,updated_at=?5 WHERE id=?1",params![command.session_id.as_str(),target,next.get() as i64,projection.get() as i64,command.now_millis])?,"session")?;
    append_intent_fact(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        Some(command.runtime_epoch),
        "sessionActivateIntent",
        &command.client_intent_id,
        projection,
        command.now_millis,
    )?;
    update_chat(
        &transaction,
        &command.project_id,
        &command.chat_id,
        projection,
        command.now_millis,
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(SessionMutation {
        session_id: command.session_id,
        runtime_epoch: Some(command.runtime_epoch),
        state: if target == "starting" {
            SessionState::Starting
        } else {
            SessionState::Recovering
        },
        revision: next,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}

fn validate_start(command: &StartSessionCommand) -> Result<(), StoreError> {
    if !valid_digest(&command.binding_digest) {
        return Err(StoreError::Corrupt("session binding"));
    }
    Ok(())
}

pub(super) fn validate_chat_revision(
    transaction: &Transaction<'_>,
    project: &SageProjectId,
    chat: &SageChatId,
    expected: ProjectionRevision,
) -> Result<(), StoreError> {
    let current: i64 = transaction
        .query_row(
            "SELECT projection_revision FROM chats WHERE project_id=?1 AND id=?2",
            params![project.as_str(), chat.as_str()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict { resource: "chat" })?;
    if current != expected.get() as i64 {
        return Err(StoreError::Conflict {
            resource: "projectionRevision",
        });
    }
    Ok(())
}

fn ensure_session_capacity(
    transaction: &Transaction<'_>,
    project: &SageProjectId,
    chat: &SageChatId,
) -> Result<(), StoreError> {
    let chat_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM agent_sessions WHERE chat_id=?1",
        [chat.as_str()],
        |r| r.get(0),
    )?;
    let project_count: i64 = transaction.query_row(
        "SELECT session_count FROM acp_project_usage WHERE project_id=?1",
        [project.as_str()],
        |r| r.get(0),
    )?;
    let global: i64 = transaction.query_row(
        "SELECT session_count FROM acp_store_usage WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    check_add(
        chat_count,
        1,
        LIMITS.durable.sessions_per_chat,
        "chatSessions",
    )?;
    check_add(
        project_count,
        1,
        LIMITS.durable.sessions_per_project,
        "projectSessions",
    )?;
    check_add(global, 1, LIMITS.durable.sessions_global, "globalSessions")
}

fn increment_session_usage(transaction: &Transaction<'_>, project: &str) -> Result<(), StoreError> {
    ensure_one(transaction.execute("UPDATE acp_project_usage SET session_count=session_count+1,revision=revision+1 WHERE project_id=?1",[project])?,"project usage")?;
    ensure_one(transaction.execute("UPDATE acp_store_usage SET session_count=session_count+1,revision=revision+1 WHERE singleton=1",[])?,"global usage")
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_intent_fact(
    transaction: &Transaction<'_>,
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    epoch: Option<RuntimeEpoch>,
    kind: &str,
    intent: &ClientIntentId,
    projection: ProjectionRevision,
    now: i64,
) -> Result<(), StoreError> {
    let payload=serde_json::to_string(&serde_json::json!({"kind":kind,"entityId":session.as_str(),"state":"committed","clientIntentId":intent.as_str(),"runtimeEpoch":epoch.map(|v|v.get())})).map_err(|_|StoreError::Corrupt("session intent serialization"))?;
    append_fact_payload(
        transaction,
        project.as_str(),
        chat.as_str(),
        session.as_str(),
        None,
        kind,
        &payload,
        projection,
        now,
    )
}

pub(super) fn update_chat(
    transaction: &Transaction<'_>,
    project: &SageProjectId,
    chat: &SageChatId,
    projection: ProjectionRevision,
    now: i64,
) -> Result<(), StoreError> {
    ensure_one(
        transaction.execute(
            "UPDATE chats SET projection_revision=?3,updated_at=?4 WHERE project_id=?1 AND id=?2",
            params![
                project.as_str(),
                chat.as_str(),
                projection.get() as i64,
                now
            ],
        )?,
        "chat",
    )
}

pub(super) fn ensure_one(changed: usize, resource: &'static str) -> Result<(), StoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(StoreError::Corrupt(resource))
    }
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
