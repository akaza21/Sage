//! A resumed session records its live negotiation; only duplicate commits
//! within the same runtime epoch must match its recorded capabilities.

use rusqlite::Connection;

use super::extended_tests::start;
use super::*;
use sage_acp_protocol::{
    contract::{ClientIntentId, RequiredText},
    intent::{CapabilitySnapshotV1, ExternalId},
    lifecycle::SessionState,
};

fn project() -> SageProjectId {
    SageProjectId::parse("project-one").unwrap()
}

fn chat() -> SageChatId {
    SageChatId::parse("chat-one").unwrap()
}

fn setup() -> Connection {
    let (connection, _) = crate::acp::migration::open_or_migrate(std::path::Path::new(":memory:"))
        .unwrap()
        .into_parts();
    connection.execute_batch(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Project',1,1,1);
         INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-one','project-one','/project','/project',0);
         INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Chat',0,1,1);
         INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision)
         VALUES('project-one',1,0,0,0,0,0,0);
         UPDATE acp_store_usage SET chat_count=1 WHERE singleton=1;",
    ).unwrap();
    connection
}

const OLD_CODEX: &str = r#"{"schemaVersion":1,"protocolVersion":"codex-app-server-observed-0.167.1","newSession":true,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":true,"promptResources":true,"promptImages":true,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}"#;
const NEW_CODEX: &str = r#"{"schemaVersion":1,"protocolVersion":"codex-app-server-observed-0.167.1","newSession":true,"resumeSession":true,"loadSession":false,"closeSession":false,"prompt":true,"promptResources":true,"promptImages":true,"cancel":true,"nativeSessionFork":true,"nativeSessionCompact":true,"nativeSessionImport":true,"nativeReview":true,"nativeBackgroundTerminals":true,"activeTurnSteer":true,"childConversationProjection":false,"childTranscriptRead":true,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}"#;

fn activate(connection: &mut Connection, session: &SageAgentSessionId, epoch: u64) {
    let expected: i64 = connection
        .query_row("SELECT projection_revision FROM chats", [], |row| {
            row.get(0)
        })
        .unwrap();
    activate_session(
        connection,
        ActivateSessionCommand {
            project_id: project(),
            chat_id: chat(),
            expected_projection_revision: ProjectionRevision::new(expected as u64).unwrap(),
            client_intent_id: ClientIntentId::parse(format!("activate-{epoch}")).unwrap(),
            session_id: session.clone(),
            runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
            artifact_digest: "d".repeat(64),
            now_millis: 20 * epoch as i64,
        },
    )
    .unwrap();
}

fn complete(
    connection: &mut Connection,
    session: &SageAgentSessionId,
    epoch: u64,
    capabilities_json: String,
) -> Result<SessionMutation, StoreError> {
    complete_activation(connection, completion(session, epoch, capabilities_json))
}

fn completion(
    session: &SageAgentSessionId,
    epoch: u64,
    capabilities_json: String,
) -> CompleteActivationCommand {
    CompleteActivationCommand {
        project_id: project(),
        chat_id: chat(),
        session_id: session.clone(),
        runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
        binding_digest: "a".repeat(64),
        artifact_digest: "d".repeat(64),
        external_session_id: ExternalId::new("external", "resumed-thread").unwrap(),
        capabilities_json,
        now_millis: 20 * epoch as i64 + 5,
    }
}

/// Starts a session with `stored` capabilities, parks it, and reactivates
/// it so the next completion is a resume.
fn parked_and_reactivated(stored: String) -> (Connection, SageAgentSessionId) {
    let mut connection = setup();
    let session = start_session(&mut connection, start(0, "start-resume"))
        .unwrap()
        .session_id;
    activate(&mut connection, &session, 1);
    complete(&mut connection, &session, 1, stored).unwrap();
    park_and_reactivate(&mut connection, &session, 1);
    (connection, session)
}

fn park_and_reactivate(connection: &mut Connection, session: &SageAgentSessionId, epoch: u64) {
    park_session_after_zero_process(
        connection,
        ParkSessionCommand {
            project_id: project(),
            chat_id: chat(),
            session_id: session.clone(),
            runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
            exit_kind: RequiredText::new("exitKind", "idleAfterTurn").unwrap(),
            now_millis: 20 * epoch as i64 + 10,
        },
    )
    .unwrap();
    activate(connection, session, epoch + 1);
}

fn stored_capabilities(connection: &Connection, session: &SageAgentSessionId) -> String {
    connection
        .query_row(
            "SELECT capabilities_json FROM agent_sessions WHERE id=?1",
            [session.as_str()],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn a_degraded_codex_session_resumes_and_records_the_live_negotiation() {
    let (mut connection, session) = parked_and_reactivated(OLD_CODEX.into());
    let resumed = complete(&mut connection, &session, 2, NEW_CODEX.into()).unwrap();
    assert_eq!(resumed.state, SessionState::Ready);
    assert_eq!(stored_capabilities(&connection, &session), NEW_CODEX);
    let page = load_chat_page(&connection, project(), chat(), None).unwrap();
    assert_eq!(
        page.sessions.as_slice()[0].capabilities,
        serde_json::from_str::<CapabilitySnapshotV1>(NEW_CODEX).unwrap()
    );
}

#[test]
fn a_capability_lost_on_resume_is_no_longer_stored_as_on() {
    let (mut connection, session) = parked_and_reactivated(NEW_CODEX.into());
    let mut negotiated: CapabilitySnapshotV1 = serde_json::from_str(NEW_CODEX).unwrap();
    negotiated.cancel = false;
    let negotiated = serde_json::to_string(&negotiated).unwrap();
    let resumed = complete(&mut connection, &session, 2, negotiated.clone()).unwrap();
    assert_eq!(resumed.state, SessionState::Ready);
    assert_eq!(stored_capabilities(&connection, &session), negotiated);
    let page = load_chat_page(&connection, project(), chat(), None).unwrap();
    assert!(!page.sessions.as_slice()[0].capabilities.cancel);
}

#[test]
fn a_codex_protocol_version_change_resumes() {
    let (mut connection, session) = parked_and_reactivated(NEW_CODEX.into());
    let negotiated = NEW_CODEX.replace("0.167.1", "0.168.0");
    let resumed = complete(&mut connection, &session, 2, negotiated.clone()).unwrap();
    assert_eq!(resumed.state, SessionState::Ready);
    assert_eq!(stored_capabilities(&connection, &session), negotiated);
}

#[test]
fn changed_model_and_config_catalog_counts_resume() {
    let (mut connection, session) = parked_and_reactivated(NEW_CODEX.into());
    let mut negotiated: CapabilitySnapshotV1 = serde_json::from_str(NEW_CODEX).unwrap();
    negotiated.model_count = 5;
    negotiated.config_count = 3;
    let negotiated = serde_json::to_string(&negotiated).unwrap();
    let resumed = complete(&mut connection, &session, 2, negotiated.clone()).unwrap();
    assert_eq!(resumed.state, SessionState::Ready);
    assert_eq!(stored_capabilities(&connection, &session), negotiated);
}

#[test]
fn a_different_external_session_id_still_conflicts() {
    let (mut connection, session) = parked_and_reactivated(OLD_CODEX.into());
    let mut command = completion(&session, 2, NEW_CODEX.into());
    command.external_session_id = ExternalId::new("external", "other-thread").unwrap();
    assert!(matches!(
        complete_activation(&mut connection, command),
        Err(StoreError::Conflict {
            resource: "activationResume"
        })
    ));
    assert_eq!(stored_capabilities(&connection, &session), OLD_CODEX);
    assert_eq!(
        complete(&mut connection, &session, 2, NEW_CODEX.into())
            .unwrap()
            .state,
        SessionState::Ready
    );
}

#[test]
fn duplicate_commits_match_only_the_running_epochs_snapshot() {
    let (mut connection, session) = parked_and_reactivated(OLD_CODEX.into());
    let resumed = complete(&mut connection, &session, 2, NEW_CODEX.into()).unwrap();
    let duplicate = complete(&mut connection, &session, 2, NEW_CODEX.into()).unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
    assert_eq!(duplicate.revision, resumed.revision);
    assert_eq!(duplicate.projection_revision, resumed.projection_revision);

    // Neither a gain nor a loss is a duplicate of this epoch's negotiation.
    for (cancel, close_session) in [(false, false), (true, true)] {
        let mut changed: CapabilitySnapshotV1 = serde_json::from_str(NEW_CODEX).unwrap();
        changed.cancel = cancel;
        changed.close_session = close_session;
        assert!(matches!(
            complete(
                &mut connection,
                &session,
                2,
                serde_json::to_string(&changed).unwrap()
            ),
            Err(StoreError::Conflict {
                resource: "activationResult"
            })
        ));
    }
    assert!(matches!(
        complete(&mut connection, &session, 2, OLD_CODEX.into()),
        Err(StoreError::Conflict {
            resource: "activationResult"
        })
    ));
    assert_eq!(stored_capabilities(&connection, &session), NEW_CODEX);
}

#[test]
fn fork_admission_follows_the_latest_negotiation_in_both_directions() {
    use sage_acp_protocol::contract::SageTurnId;

    let (mut connection, session) = parked_and_reactivated(OLD_CODEX.into());
    complete(&mut connection, &session, 2, NEW_CODEX.into()).unwrap();
    connection
        .execute(
            "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,
           client_intent_id,state,delivery_state,revision,created_at,updated_at,terminal_at)
         VALUES('turn-fork','project-one','chat-one',?1,2,0,'intent-fork','completed',
           'correlated',0,46,47,47)",
            [session.as_str()],
        )
        .unwrap();
    let turn = SageTurnId::parse("turn-fork").unwrap();
    let source = chat_fork::fork_source(&connection, &project(), &chat(), &turn).unwrap();
    assert_eq!(source.session_id, session);
    assert_eq!(source.runtime_epoch, RuntimeEpoch::new(2).unwrap());

    park_and_reactivate(&mut connection, &session, 2);
    let mut negotiated: CapabilitySnapshotV1 = serde_json::from_str(NEW_CODEX).unwrap();
    negotiated.native_session_fork = false;
    complete(
        &mut connection,
        &session,
        3,
        serde_json::to_string(&negotiated).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        chat_fork::fork_source(&connection, &project(), &chat(), &turn),
        Err(StoreError::Conflict {
            resource: "forkCapability"
        })
    ));
}

#[test]
fn an_invalid_negotiation_cannot_replace_the_stored_snapshot() {
    let (mut connection, session) = parked_and_reactivated(OLD_CODEX.into());
    for invalid in [
        "not json".into(),
        NEW_CODEX.replace("\"cancel\":true", "\"cancel\":\"yes\""),
        NEW_CODEX.replace("\"schemaVersion\":1", "\"schemaVersion\":2"),
        NEW_CODEX.replace("\"modelCount\":0", "\"modelCount\":129"),
    ] {
        assert!(matches!(
            complete(&mut connection, &session, 2, invalid),
            Err(StoreError::Corrupt("activation result"))
        ));
        assert_eq!(stored_capabilities(&connection, &session), OLD_CODEX);
    }
    assert_eq!(
        complete(&mut connection, &session, 2, NEW_CODEX.into())
            .unwrap()
            .state,
        SessionState::Ready
    );
}

#[test]
fn resume_cannot_mutate_native_identity_or_accept_invalid_json() {
    let (mut connection, session) = parked_and_reactivated(OLD_CODEX.into());
    for assignment in [
        "external_session_id='other-thread'",
        "harness_version='2.0.0'",
        "binding_digest=printf('%064d', 0)",
        "binding_id='other-binding'",
        "model_id='other-model'",
        "created_at=0",
        "legacy=1",
        "ordinal=1",
    ] {
        let error = connection
            .execute(
                &format!(
                    "UPDATE agent_sessions SET state='ready',capabilities_json=?1,{assignment}
                     WHERE id=?2"
                ),
                [NEW_CODEX, session.as_str()],
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("agent session identity is immutable"),
            "{assignment}: {error}"
        );
        assert_eq!(stored_capabilities(&connection, &session), OLD_CODEX);
    }
    let error = connection
        .execute(
            "UPDATE agent_sessions SET state='ready',capabilities_json='not json' WHERE id=?1",
            [session.as_str()],
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("agent session identity is immutable")
    );
    assert_eq!(stored_capabilities(&connection, &session), OLD_CODEX);
    assert_eq!(
        complete(&mut connection, &session, 2, NEW_CODEX.into())
            .unwrap()
            .state,
        SessionState::Ready
    );
}

#[test]
fn capabilities_cannot_change_without_a_resume_commit() {
    let (mut connection, session) = parked_and_reactivated(OLD_CODEX.into());
    let error = connection
        .execute(
            "UPDATE agent_sessions SET capabilities_json=?1 WHERE id=?2",
            [NEW_CODEX, session.as_str()],
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("agent session identity is immutable")
    );
    complete(&mut connection, &session, 2, NEW_CODEX.into()).unwrap();
    let error = connection
        .execute(
            "UPDATE agent_sessions SET capabilities_json=?1 WHERE id=?2",
            [OLD_CODEX, session.as_str()],
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("agent session identity is immutable")
    );
    assert_eq!(stored_capabilities(&connection, &session), NEW_CODEX);
}
