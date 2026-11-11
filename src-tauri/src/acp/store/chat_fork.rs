//! Durable native-fork lineage. Rust alone decides whether a Chat may be
//! forked, then installs the fork's resumable session head and the immutable
//! `chat_lineage` row in the same transaction that creates the fork Chat.

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::Value;

use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId},
    intent::ExternalId,
};

use super::{
    StoreError,
    facts::{append_fact_payload, next_projection_revision},
    session_head::{HeadSession, install_head_session},
    sessions::{ensure_one, ensure_session_capacity, update_chat},
};

const FORK_CAPABILITY: &str = "nativeSessionFork";

/// Capabilities a harness may gain once Sage learns to drive them, with
/// whether a snapshot records them when false.
const UPGRADES: [(&str, bool); 4] = [
    (FORK_CAPABILITY, true),
    ("activeTurnSteer", true),
    ("nativeSessionCompact", false),
    ("promptImages", false),
];

/// A session negotiated before Sage drove one of its harness's methods
/// stored that capability as off; the same harness now negotiates it on.
/// Such upgrades still resume the session. Its stored snapshot stays
/// immutable, so the session keeps reporting them off (fail closed).
pub(crate) fn resumable_capabilities(stored: &str, negotiated: &str) -> bool {
    if stored == negotiated {
        return true;
    }
    let (Ok(Value::Object(mut stored)), Ok(Value::Object(mut negotiated))) = (
        serde_json::from_str::<Value>(stored),
        serde_json::from_str::<Value>(negotiated),
    ) else {
        return false;
    };
    UPGRADES.iter().all(|(key, recorded)| {
        let (was, now) = (stored.remove(*key), negotiated.remove(*key));
        let off = match was {
            Some(Value::Bool(false)) => true,
            None => !recorded,
            _ => false,
        };
        was == now || (off && now == Some(Value::Bool(true)))
    }) && stored == negotiated
}

/// The Turn a fork is taken through: the latest completed Turn of the Chat's
/// live head session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForkSource {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
}

/// Admits a fork only from the latest completed Turn of a hot head session
/// whose own durable capabilities admit `nativeSessionFork`, with no Turn in
/// flight. Temporary side chats are never fork sources.
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
    let admits_fork = serde_json::from_str::<Value>(&capabilities)
        .ok()
        .and_then(|value| value.get(FORK_CAPABILITY).and_then(Value::as_bool))
        == Some(true);
    if !admits_fork || residency != "hot" || !matches!(state.as_str(), "ready" | "active") {
        return Err(StoreError::Conflict {
            resource: "forkCapability",
        });
    }
    require_latest_completed(connection, &session, turn_id)?;
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
    })
}

fn require_latest_completed(
    connection: &Connection,
    session: &str,
    turn_id: &SageTurnId,
) -> Result<(), StoreError> {
    let latest: Option<(String, String)> = connection
        .query_row(
            "SELECT id,state FROM turns WHERE session_id=?1 ORDER BY ordinal DESC LIMIT 1",
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

#[cfg(test)]
mod tests {
    use super::resumable_capabilities;

    const STORED: &str =
        r#"{"schemaVersion":1,"protocolVersion":"p","prompt":true,"nativeSessionFork":false}"#;

    #[test]
    fn only_the_fork_upgrade_of_an_otherwise_equal_snapshot_still_resumes() {
        assert!(resumable_capabilities(STORED, STORED));
        assert!(resumable_capabilities(
            STORED,
            r#"{"schemaVersion":1,"protocolVersion":"p","prompt":true,"nativeSessionFork":true}"#
        ));
        // A downgrade, any other drift, or garbage is still a conflict.
        let forkable = STORED.replace("false", "true");
        assert!(!resumable_capabilities(&forkable, STORED));
        assert!(!resumable_capabilities(
            STORED,
            r#"{"schemaVersion":1,"protocolVersion":"p","prompt":false,"nativeSessionFork":true}"#
        ));
        assert!(!resumable_capabilities(STORED, "not json"));
    }

    #[test]
    fn steer_and_compact_upgrades_still_resume_but_never_a_downgrade() {
        let stored = r#"{"schemaVersion":1,"prompt":true,"nativeSessionFork":false,"activeTurnSteer":false}"#;
        let upgraded = r#"{"schemaVersion":1,"prompt":true,"nativeSessionFork":false,"nativeSessionCompact":true,"activeTurnSteer":true}"#;
        assert!(resumable_capabilities(stored, upgraded));
        assert!(!resumable_capabilities(upgraded, stored));
        // A recorded flag cannot go missing.
        assert!(!resumable_capabilities(
            stored,
            r#"{"schemaVersion":1,"prompt":true,"nativeSessionFork":false}"#
        ));
        assert!(!resumable_capabilities(
            stored,
            r#"{"schemaVersion":1,"prompt":true,"nativeSessionFork":false,"activeTurnSteer":"yes"}"#
        ));
    }

    #[test]
    fn a_session_from_before_prompt_images_still_resumes_with_them() {
        let stored = r#"{"schemaVersion":1,"prompt":true,"nativeSessionFork":true}"#;
        let upgraded =
            r#"{"schemaVersion":1,"prompt":true,"nativeSessionFork":true,"promptImages":true}"#;
        assert!(resumable_capabilities(stored, upgraded));
        assert!(!resumable_capabilities(upgraded, stored));
    }
}
