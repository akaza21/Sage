//! Durable native-fork lineage. Rust alone decides whether a Chat may be
//! forked, then installs the fork's resumable session head and the immutable
//! `chat_lineage` row in the same transaction that creates the fork Chat.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId},
    intent::{CapabilitySnapshotV1, ExternalId},
};

use super::{
    StoreError,
    facts::{append_fact_payload, next_projection_revision},
    session_head::{HeadSession, install_head_session},
    sessions::{ensure_one, ensure_session_capacity, update_chat},
};

/// Whether a session stored with the `stored` capability snapshot resumes
/// under the `negotiated` one; see [`CapabilitySnapshotV1::resumes_as`].
/// A snapshot that does not decode is a conflict.
pub(crate) fn resumable_capabilities(stored: &str, negotiated: &str) -> bool {
    if stored == negotiated {
        return true;
    }
    match (
        serde_json::from_str::<CapabilitySnapshotV1>(stored),
        serde_json::from_str::<CapabilitySnapshotV1>(negotiated),
    ) {
        (Ok(stored), Ok(negotiated)) => stored.resumes_as(&negotiated),
        _ => false,
    }
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
    let admits_fork = serde_json::from_str::<CapabilitySnapshotV1>(&capabilities)
        .is_ok_and(|snapshot| snapshot.native_session_fork);
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
    use serde_json::{Value, json};

    use super::resumable_capabilities;

    /// A snapshot as a build that predates every optional capability stored
    /// it: optional flags that were off are absent.
    fn stored() -> Value {
        json!({
            "schemaVersion": 1,
            "protocolVersion": "p",
            "newSession": true,
            "resumeSession": true,
            "loadSession": false,
            "closeSession": true,
            "prompt": true,
            "cancel": true,
            "nativeSessionFork": false,
            "activeTurnSteer": false,
            "childConversationProjection": false,
            "childTranscriptRead": false,
            "collaborationEventProjection": false,
            "multiRootExtensionSchemaId": null,
            "modelCount": 1,
            "configCount": 0
        })
    }

    fn with(mut snapshot: Value, key: &str, value: Value) -> Value {
        snapshot[key] = value;
        snapshot
    }

    fn resumes(stored: &Value, negotiated: &Value) -> bool {
        resumable_capabilities(&stored.to_string(), &negotiated.to_string())
    }

    const UPGRADES: [&str; 8] = [
        "promptResources",
        "promptImages",
        "nativeSessionFork",
        "nativeSessionCompact",
        "nativeSessionImport",
        "nativeReview",
        "nativeBackgroundTerminals",
        "activeTurnSteer",
    ];

    #[test]
    fn every_capability_upgrade_resumes_but_never_a_downgrade() {
        assert!(resumes(&stored(), &stored()));
        for key in UPGRADES {
            let upgraded = with(stored(), key, json!(true));
            assert!(resumes(&stored(), &upgraded), "{key} upgrade must resume");
            assert!(!resumes(&upgraded, &stored()), "{key} downgrade must not");
            // An explicit off resumes like the absent field it replaces.
            let explicit = with(stored(), key, json!(false));
            assert!(resumes(&explicit, &upgraded), "{key} from explicit off");
        }
        let all = UPGRADES
            .iter()
            .fold(stored(), |snapshot, key| with(snapshot, key, json!(true)));
        assert!(resumes(&stored(), &all));
    }

    #[test]
    fn any_change_to_a_fixed_fact_is_a_conflict_even_beside_an_upgrade() {
        let changes = [
            ("protocolVersion", json!("q")),
            ("newSession", json!(false)),
            ("resumeSession", json!(false)),
            ("loadSession", json!(true)),
            ("closeSession", json!(false)),
            ("prompt", json!(false)),
            ("cancel", json!(false)),
            ("childConversationProjection", json!(true)),
            ("childTranscriptRead", json!(true)),
            ("collaborationEventProjection", json!(true)),
            ("multiRootExtensionSchemaId", json!("roots")),
            ("modelCount", json!(2)),
            ("configCount", json!(1)),
        ];
        for (key, value) in changes {
            let changed = with(stored(), key, value);
            assert!(!resumes(&stored(), &changed), "{key} change must conflict");
            let upgraded = with(changed, "nativeBackgroundTerminals", json!(true));
            assert!(!resumes(&stored(), &upgraded), "{key} beside an upgrade");
        }
    }

    #[test]
    fn a_missing_required_flag_a_malformed_value_or_garbage_is_a_conflict() {
        let mut missing = stored();
        missing.as_object_mut().unwrap().remove("activeTurnSteer");
        assert!(!resumes(&stored(), &missing));
        assert!(!resumes(&missing, &stored()));
        assert!(!resumes(
            &stored(),
            &with(stored(), "activeTurnSteer", json!("yes"))
        ));
        // A snapshot stored by a newer build names a capability this one
        // does not know.
        assert!(!resumes(
            &with(stored(), "unknownFlag", json!(false)),
            &stored()
        ));
        assert!(!resumable_capabilities(&stored().to_string(), "not json"));
    }
}
