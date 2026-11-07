use rusqlite::params;

use super::*;
use crate::{
    acp::{
        contract::{ClientIntentId, SchemaVersionV1},
        lifecycle::{TurnState, TurnTerminalOutcome, TurnTerminalReason},
        projection::TurnTerminalReasonProjection,
    },
    persistence::Database,
};

fn seeded() -> Database {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let connection = database.connection();
    connection
        .execute_batch(&format!(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Terminal',1,1,1);
             INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-one','project-one','C:/fixture','c:/fixture',0);
             INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Terminal',0,1,1);
             INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-one','project-one','chat-one',0,'binding','{digest}','external-session','{capabilities}','active','hot',0,0,0,1,1);
             INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1);
             INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-one','chat-one','session-one',1,'running','{artifact}',1);
             INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-one','project-one','chat-one','session-one',1,0,'intent-one','sending','notWritten',0,1,1);
             INSERT INTO acp_project_usage VALUES('project-one',1,0,1,0,0,0,0);
             INSERT INTO acp_session_usage VALUES('project-one','chat-one','session-one',1,0,0,0,0,0,0);
             UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=1 WHERE singleton=1;",
            digest = "07".repeat(32),
            artifact = "d".repeat(64),
            capabilities = serde_json::to_string(&crate::acp::intent::CapabilitySnapshotV1 {
                schema_version: SchemaVersionV1,
                protocol_version: crate::acp::contract::RequiredText::new("protocol", "1").unwrap(),
                new_session: true,
                resume_session: false,
                load_session: false,
                close_session: true,
                prompt: true,
                cancel: true,
                native_session_fork: false,
                active_turn_steer: false,
                child_conversation_projection: false,
                child_transcript_read: false,
                collaboration_event_projection: false,
                multi_root_extension_schema_id: None,
                model_count: 0,
                prompt_resources: false,
                config_count: 0,
            })
            .unwrap(),
        ))
        .unwrap();
    drop(connection);
    database
}

fn turn_command(revision: u64, now_millis: i64) -> TurnMutationCommand {
    TurnMutationCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        turn_id: SageTurnId::parse("turn-one").unwrap(),
        expected_revision: EntityRevision::new(revision).unwrap(),
        now_millis,
    }
}

fn projected(
    connection: &rusqlite::Connection,
) -> (TurnState, Option<TurnTerminalReasonProjection>) {
    let page = load_chat_page(
        connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    let turn = page.turns.as_slice().first().cloned().unwrap();
    (turn.state, turn.terminal_reason)
}

#[test]
fn a_failed_turn_without_evidence_projects_the_other_reason() {
    let database = seeded();
    let mut connection = database.connection();
    let mutation = record_send_observation(
        &mut connection,
        turn_command(0, 20),
        SendObservation::ProcessLost,
    )
    .unwrap();
    assert_eq!(mutation.state, TurnState::Failed);
    assert_eq!(
        projected(&connection),
        (
            TurnState::Failed,
            Some(TurnTerminalReasonProjection {
                kind: TurnTerminalReason::Other,
                message: None,
                remediation: None,
            })
        )
    );
}

#[test]
fn a_protocol_failure_projects_the_protocol_error_reason() {
    let database = seeded();
    let mut connection = database.connection();
    let commit = commit_batch(
        &mut connection,
        &[StoreMutation::ObserveSend {
            command: turn_command(0, 20),
            observation: SendObservation::ProcessLost,
            terminal_reason: Some(TurnTerminalReason::ProtocolError.into()),
        }],
    )
    .unwrap();
    assert_eq!(commit.results.len(), 1);
    assert_eq!(
        projected(&connection),
        (
            TurnState::Failed,
            Some(TurnTerminalReasonProjection {
                kind: TurnTerminalReason::ProtocolError,
                message: None,
                remediation: None,
            })
        )
    );
}

#[test]
fn a_harness_exit_projects_its_exit_code_as_the_reason_message() {
    let database = seeded();
    let mut connection = database.connection();
    let commit = commit_batch(
        &mut connection,
        &[StoreMutation::ObserveSend {
            command: turn_command(0, 20),
            observation: SendObservation::ProcessLost,
            terminal_reason: Some(TurnTerminalOutcome {
                reason: TurnTerminalReason::HarnessExited,
                message: Some("harness process exited with code 17".to_owned()),
                remediation: None,
            }),
        }],
    )
    .unwrap();
    assert_eq!(commit.results.len(), 1);
    assert_eq!(
        projected(&connection),
        (
            TurnState::Failed,
            Some(TurnTerminalReasonProjection {
                kind: TurnTerminalReason::HarnessExited,
                message: Some("harness process exited with code 17".to_owned()),
                remediation: None,
            })
        )
    );
}

#[test]
fn a_protocol_failure_projects_its_summary_as_the_reason_message() {
    let database = seeded();
    let mut connection = database.connection();
    commit_batch(
        &mut connection,
        &[StoreMutation::ObserveSend {
            command: turn_command(0, 20),
            observation: SendObservation::ProcessLost,
            terminal_reason: Some(TurnTerminalOutcome {
                reason: TurnTerminalReason::ProtocolError,
                message: Some("harness sent a malformed protocol frame".to_owned()),
                remediation: None,
            }),
        }],
    )
    .unwrap();
    assert_eq!(
        projected(&connection).1.and_then(|reason| reason.message),
        Some("harness sent a malformed protocol frame".to_owned())
    );
}

#[test]
fn a_cancelled_turn_without_evidence_projects_the_other_reason() {
    let database = seeded();
    let mut connection = database.connection();
    let cancelling = begin_cancel(
        &mut connection,
        BeginCancelCommand {
            turn: turn_command(0, 20),
            client_intent_id: ClientIntentId::parse("cancel-one").unwrap(),
        },
    )
    .unwrap();
    assert_eq!(cancelling.state, TurnState::Cancelling);
    let cancelled = record_send_observation(
        &mut connection,
        turn_command(cancelling.revision.get(), 21),
        SendObservation::ProcessLost,
    )
    .unwrap();
    assert_eq!(cancelled.state, TurnState::Cancelled);
    assert_eq!(
        projected(&connection),
        (
            TurnState::Cancelled,
            Some(TurnTerminalReasonProjection {
                kind: TurnTerminalReason::Other,
                message: None,
                remediation: None,
            })
        )
    );
}

#[test]
fn a_host_terminated_cancel_projects_the_cancelled_by_user_reason() {
    let database = seeded();
    let mut connection = database.connection();
    let cancelling = begin_cancel(
        &mut connection,
        BeginCancelCommand {
            turn: turn_command(0, 20),
            client_intent_id: ClientIntentId::parse("cancel-one").unwrap(),
        },
    )
    .unwrap();
    let commit = commit_batch(
        &mut connection,
        &[StoreMutation::ObserveSend {
            command: turn_command(cancelling.revision.get(), 21),
            observation: SendObservation::ProcessLost,
            terminal_reason: Some(TurnTerminalReason::CancelledByUser.into()),
        }],
    )
    .unwrap();
    assert_eq!(commit.results.len(), 1);
    assert_eq!(
        projected(&connection),
        (
            TurnState::Cancelled,
            Some(TurnTerminalReasonProjection {
                kind: TurnTerminalReason::CancelledByUser,
                message: None,
                remediation: None,
            })
        )
    );
}

#[test]
fn a_repeated_reason_write_keeps_the_first_reason() {
    let database = seeded();
    let mut connection = database.connection();
    let transaction = connection.transaction().unwrap();
    let command = turn_command(0, 20);
    assert!(
        terminal_reason::record_in(
            &transaction,
            (&command).into(),
            TurnTerminalReason::AuthRequired,
            Some("sign in"),
            Some("run /login"),
            20,
        )
        .unwrap()
    );
    assert!(
        !terminal_reason::record_in(
            &transaction,
            (&command).into(),
            TurnTerminalReason::Other,
            None,
            None,
            21,
        )
        .unwrap()
    );
    let stored: (String, Option<String>, Option<String>) = transaction
        .query_row(
            "SELECT kind,message,remediation FROM acp_turn_terminal_reasons WHERE turn_id='turn-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        stored,
        (
            "authRequired".to_owned(),
            Some("sign in".to_owned()),
            Some("run /login".to_owned())
        )
    );
}

#[test]
fn a_remediation_survives_the_round_trip_beside_its_message() {
    let database = seeded();
    let mut connection = database.connection();
    commit_batch(
        &mut connection,
        &[StoreMutation::ObserveSend {
            command: turn_command(0, 20),
            observation: SendObservation::ProcessLost,
            terminal_reason: Some(TurnTerminalOutcome {
                reason: TurnTerminalReason::AuthRequired,
                message: Some("authentication required".to_owned()),
                remediation: Some("Sign in by running claude auth login.".to_owned()),
            }),
        }],
    )
    .unwrap();
    assert_eq!(
        projected(&connection).1,
        Some(TurnTerminalReasonProjection {
            kind: TurnTerminalReason::AuthRequired,
            message: Some("authentication required".to_owned()),
            remediation: Some("Sign in by running claude auth login.".to_owned()),
        })
    );
}

#[test]
fn an_oversized_reason_remediation_is_rejected() {
    let database = seeded();
    let mut connection = database.connection();
    let transaction = connection.transaction().unwrap();
    let command = turn_command(0, 20);
    assert!(matches!(
        terminal_reason::record_in(
            &transaction,
            (&command).into(),
            TurnTerminalReason::Other,
            None,
            Some(&"r".repeat(8193)),
            20,
        ),
        Err(StoreError::Capacity { .. })
    ));
}

#[test]
fn an_oversized_reason_message_is_rejected() {
    let database = seeded();
    let mut connection = database.connection();
    let transaction = connection.transaction().unwrap();
    let command = turn_command(0, 20);
    assert!(matches!(
        terminal_reason::record_in(
            &transaction,
            (&command).into(),
            TurnTerminalReason::Other,
            Some(&"m".repeat(8193)),
            None,
            20,
        ),
        Err(StoreError::Capacity { .. })
    ));
}

#[test]
fn a_live_turn_never_projects_a_terminal_reason() {
    let database = seeded();
    let connection = database.connection();
    connection
        .execute(
            "INSERT INTO acp_turn_terminal_reasons
             (project_id,chat_id,session_id,turn_id,runtime_epoch,kind,message,created_at)
             VALUES('project-one','chat-one','session-one','turn-one',1,'other',NULL,1)",
            params![],
        )
        .unwrap();
    assert_eq!(projected(&connection), (TurnState::Sending, None));
}
