//! A parked session resumes under the capabilities its harness negotiates
//! now, as long as they differ from the stored ones only by upgrades.

use rusqlite::Connection;

use super::extended_tests::{capabilities, setup, start};
use super::*;
use crate::acp::{
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

fn with_background_terminals(capabilities: &str, on: bool) -> String {
    let mut snapshot: CapabilitySnapshotV1 = serde_json::from_str(capabilities).unwrap();
    snapshot.native_background_terminals = on;
    serde_json::to_string(&snapshot).unwrap()
}

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
    complete_activation(
        connection,
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
        },
    )
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
    park_session_after_zero_process(
        &mut connection,
        ParkSessionCommand {
            project_id: project(),
            chat_id: chat(),
            session_id: session.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            exit_kind: RequiredText::new("exitKind", "idleAfterTurn").unwrap(),
            now_millis: 30,
        },
    )
    .unwrap();
    activate(&mut connection, &session, 2);
    (connection, session)
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
fn a_session_stored_before_a_capability_existed_resumes_once_it_is_negotiated() {
    // A build that predates the capability omits its field entirely.
    let stored = capabilities();
    assert!(!stored.contains("nativeBackgroundTerminals"));
    let (mut connection, session) = parked_and_reactivated(stored.clone());
    let resumed = complete(
        &mut connection,
        &session,
        2,
        with_background_terminals(&stored, true),
    )
    .unwrap();
    assert_eq!(resumed.state, SessionState::Ready);
    // The stored snapshot stays as negotiated first, so the upgrade stays off.
    assert_eq!(stored_capabilities(&connection, &session), stored);
}

#[test]
fn a_session_never_resumes_without_a_capability_it_was_stored_with() {
    let stored = with_background_terminals(&capabilities(), true);
    let (mut connection, session) = parked_and_reactivated(stored);
    assert!(matches!(
        complete(&mut connection, &session, 2, capabilities()),
        Err(StoreError::Conflict {
            resource: "activationResume"
        })
    ));
}
