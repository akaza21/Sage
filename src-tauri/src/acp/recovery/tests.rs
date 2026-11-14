use super::*;
use rusqlite::Connection;
use sage_acp_protocol::lifecycle::TurnTerminalReason;

#[test]
fn restart_never_replays_a_prompt() {
    let cases = [
        (
            (TurnState::Queued, DeliveryState::NotWritten),
            TurnState::Cancelled,
        ),
        (
            (TurnState::Sending, DeliveryState::ZeroBytes),
            TurnState::Failed,
        ),
        (
            (TurnState::Sending, DeliveryState::Writing),
            TurnState::DeliveryUnknown,
        ),
        (
            (TurnState::Sending, DeliveryState::CompleteBytes),
            TurnState::DeliveryUnknown,
        ),
        (
            (TurnState::Running, DeliveryState::Correlated),
            TurnState::Failed,
        ),
        (
            (TurnState::WaitingTool, DeliveryState::Correlated),
            TurnState::Failed,
        ),
    ];
    for ((state, delivery), expected) in cases {
        assert_eq!(recovery_turn_state(state, delivery).unwrap().0, expected);
    }
}

#[test]
fn impossible_recovery_pair_is_corruption() {
    assert!(recovery_turn_state(TurnState::Queued, DeliveryState::Correlated).is_err());
}

#[test]
fn restart_recovery_commits_delivery_unknown_and_converges_to_cold_zero_runtime() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(
        "CREATE TABLE chats(id TEXT PRIMARY KEY, project_id TEXT, projection_revision INTEGER, updated_at INTEGER);
         CREATE TABLE agent_sessions(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, state TEXT, residency TEXT, revision INTEGER, projection_revision INTEGER, updated_at INTEGER, terminal_at INTEGER);
         CREATE TABLE chat_session_heads(session_id TEXT PRIMARY KEY);
         CREATE TABLE session_runtime_epochs(session_id TEXT, runtime_epoch INTEGER, state TEXT, terminal_at INTEGER, exit_kind TEXT, started_at INTEGER, PRIMARY KEY(session_id,runtime_epoch));
         CREATE TABLE turns(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, session_id TEXT, runtime_epoch INTEGER, ordinal INTEGER NOT NULL, client_intent_id TEXT, state TEXT, delivery_state TEXT, revision INTEGER, updated_at INTEGER, terminal_at INTEGER);
         CREATE TABLE items(id TEXT PRIMARY KEY,session_id TEXT,runtime_epoch INTEGER,kind TEXT,lifecycle TEXT,tool_state TEXT,revision INTEGER,updated_at INTEGER,terminal_at INTEGER,turn_id TEXT,ordinal INTEGER,payload_json TEXT);
         CREATE TABLE session_activities(id TEXT PRIMARY KEY,session_id TEXT,runtime_epoch INTEGER,lifecycle TEXT,revision INTEGER,updated_at INTEGER,terminal_at INTEGER);
         CREATE TABLE server_requests(id TEXT PRIMARY KEY,session_id TEXT,runtime_epoch INTEGER,state TEXT,revision INTEGER,updated_at INTEGER,resolved_at INTEGER);
         CREATE TABLE acp_turn_terminal_reasons(project_id TEXT,chat_id TEXT,session_id TEXT,turn_id TEXT,runtime_epoch INTEGER,kind TEXT,message TEXT,remediation TEXT,created_at INTEGER,PRIMARY KEY(session_id,runtime_epoch,turn_id));
         CREATE TABLE session_facts(project_id TEXT, chat_id TEXT, session_id TEXT, sequence INTEGER, turn_id TEXT, kind TEXT, payload_json TEXT, payload_bytes INTEGER, payload_digest TEXT, projection_revision INTEGER, created_at INTEGER, PRIMARY KEY(session_id,sequence));
         CREATE TABLE acp_session_usage(session_id TEXT PRIMARY KEY, fact_count INTEGER, fact_bytes INTEGER, payload_bytes INTEGER, revision INTEGER);
         CREATE TABLE acp_project_usage(project_id TEXT PRIMARY KEY, fact_count INTEGER, payload_bytes INTEGER, revision INTEGER);
         CREATE TABLE acp_store_usage(singleton INTEGER PRIMARY KEY, fact_count INTEGER, fact_bytes INTEGER, shared_payload_bytes INTEGER, revision INTEGER);
         INSERT INTO chats VALUES('chat-one','project-one',0,0);
         INSERT INTO agent_sessions VALUES('session-one','project-one','chat-one','active','hot',0,0,0,NULL);
         INSERT INTO chat_session_heads VALUES('session-one');
         INSERT INTO session_runtime_epochs VALUES('session-one',1,'running',NULL,NULL,0);
         INSERT INTO turns VALUES('turn-one','project-one','chat-one','session-one',1,0,'intent-one','sending','writing',0,0,NULL);
         INSERT INTO items VALUES('item-one','session-one',1,'permissionRequest','streaming','waitingForApproval',0,0,NULL,'turn-one',0,'{}');
         INSERT INTO session_activities VALUES('activity-one','session-one',1,'active',0,0,NULL);
         INSERT INTO server_requests VALUES('request-one','session-one',1,'received',0,0,NULL);
         INSERT INTO acp_session_usage VALUES('session-one',0,0,0,0);
         INSERT INTO acp_project_usage VALUES('project-one',0,0,0);
         INSERT INTO acp_store_usage VALUES(1,0,0,0,0);",
    ).unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/019_turn_workspace_changes.sql"
        ))
        .unwrap();

    let report = recover_after_restart(&mut connection, 50).unwrap();
    assert_eq!(report.recovered_sessions, 1);
    assert_eq!(
        report.terminal_turn,
        Some((
            SageTurnId::parse("turn-one").unwrap(),
            TurnState::DeliveryUnknown
        ))
    );
    let turn: (String, String) = connection
        .query_row(
            "SELECT state,delivery_state FROM turns WHERE id='turn-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(turn, ("deliveryUnknown".into(), "possibleBytes".into()));
    let session: (String, String) = connection
        .query_row(
            "SELECT state,residency FROM agent_sessions WHERE id='session-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(session, ("failed".into(), "cold".into()));
    let epoch: (String, String) = connection
        .query_row(
            "SELECT state,exit_kind FROM session_runtime_epochs WHERE session_id='session-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(epoch, ("failed".into(), "restartRecovery".into()));
    let heads: i64 = connection
        .query_row("SELECT COUNT(*) FROM chat_session_heads", [], |row| {
            row.get(0)
        })
        .unwrap();
    let facts: i64 = connection
        .query_row("SELECT COUNT(*) FROM session_facts", [], |row| row.get(0))
        .unwrap();
    assert_eq!((heads, facts), (0, 1));
    let descendants: (String, String, String) = connection
        .query_row(
            "SELECT i.lifecycle,a.lifecycle,r.state FROM items i
             CROSS JOIN session_activities a CROSS JOIN server_requests r",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        descendants,
        (
            "failed".into(),
            "cancelled".into(),
            "resolvedDeclined".into()
        )
    );
}

#[test]
fn exact_zero_process_recovery_replays_only_the_proven_terminal_postcondition() {
    let mut connection = recovery_connection();
    let command = RecoverRuntimeCommand {
        project_id: sage_acp_protocol::contract::SageProjectId::parse("project-one").unwrap(),
        chat_id: sage_acp_protocol::contract::SageChatId::parse("chat-one").unwrap(),
        session_id: sage_acp_protocol::contract::SageAgentSessionId::parse("session-one").unwrap(),
        runtime_epoch: sage_acp_protocol::contract::RuntimeEpoch::new(1).unwrap(),
        terminal_reason: TurnTerminalReason::HarnessExited.into(),
        now_millis: 50,
    };
    let first = recover_runtime_after_zero_process(&mut connection, command.clone()).unwrap();
    assert_eq!(first.recovered_sessions, 1);
    let replay = recover_runtime_after_zero_process(&mut connection, command.clone()).unwrap();
    assert_eq!(replay.recovered_sessions, 0);
    assert_eq!(replay.projection_revision, first.projection_revision);

    let mut missing = command;
    missing.session_id =
        sage_acp_protocol::contract::SageAgentSessionId::parse("session-missing").unwrap();
    assert!(matches!(
        recover_runtime_after_zero_process(&mut connection, missing),
        Err(StoreError::Conflict {
            resource: "runtimeRecovery"
        })
    ));
}

#[test]
fn zero_process_recovery_rejects_a_foreign_live_tree() {
    let mut connection = recovery_connection();
    let command = RecoverRuntimeCommand {
        project_id: sage_acp_protocol::contract::SageProjectId::parse("project-one").unwrap(),
        chat_id: sage_acp_protocol::contract::SageChatId::parse("chat-one").unwrap(),
        session_id: sage_acp_protocol::contract::SageAgentSessionId::parse("session-foreign")
            .unwrap(),
        runtime_epoch: sage_acp_protocol::contract::RuntimeEpoch::new(1).unwrap(),
        terminal_reason: TurnTerminalReason::HarnessExited.into(),
        now_millis: 50,
    };
    assert!(matches!(
        recover_runtime_after_zero_process(&mut connection, command),
        Err(StoreError::Conflict {
            resource: "runtimeRecovery"
        })
    ));
    let state: String = connection
        .query_row(
            "SELECT state FROM session_runtime_epochs WHERE session_id='session-one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "running");
}

fn recovery_connection() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(
        "CREATE TABLE chats(id TEXT PRIMARY KEY, project_id TEXT, projection_revision INTEGER, updated_at INTEGER);
         CREATE TABLE agent_sessions(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, state TEXT, residency TEXT, revision INTEGER, projection_revision INTEGER, updated_at INTEGER, terminal_at INTEGER);
         CREATE TABLE chat_session_heads(session_id TEXT PRIMARY KEY);
         CREATE TABLE session_runtime_epochs(session_id TEXT, runtime_epoch INTEGER, state TEXT, terminal_at INTEGER, exit_kind TEXT, started_at INTEGER, PRIMARY KEY(session_id,runtime_epoch));
         CREATE TABLE turns(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, session_id TEXT, runtime_epoch INTEGER, ordinal INTEGER NOT NULL, client_intent_id TEXT, state TEXT, delivery_state TEXT, revision INTEGER, updated_at INTEGER, terminal_at INTEGER);
         CREATE TABLE items(id TEXT PRIMARY KEY,session_id TEXT,runtime_epoch INTEGER,kind TEXT,lifecycle TEXT,tool_state TEXT,revision INTEGER,updated_at INTEGER,terminal_at INTEGER,turn_id TEXT,ordinal INTEGER,payload_json TEXT);
         CREATE TABLE session_activities(id TEXT PRIMARY KEY,session_id TEXT,runtime_epoch INTEGER,lifecycle TEXT,revision INTEGER,updated_at INTEGER,terminal_at INTEGER);
         CREATE TABLE server_requests(id TEXT PRIMARY KEY,session_id TEXT,runtime_epoch INTEGER,state TEXT,revision INTEGER,updated_at INTEGER,resolved_at INTEGER);
         CREATE TABLE acp_turn_terminal_reasons(project_id TEXT,chat_id TEXT,session_id TEXT,turn_id TEXT,runtime_epoch INTEGER,kind TEXT,message TEXT,remediation TEXT,created_at INTEGER,PRIMARY KEY(session_id,runtime_epoch,turn_id));
         CREATE TABLE session_facts(project_id TEXT, chat_id TEXT, session_id TEXT, sequence INTEGER, turn_id TEXT, kind TEXT, payload_json TEXT, payload_bytes INTEGER, payload_digest TEXT, projection_revision INTEGER, created_at INTEGER, PRIMARY KEY(session_id,sequence));
         CREATE TABLE acp_session_usage(session_id TEXT PRIMARY KEY, fact_count INTEGER, fact_bytes INTEGER, payload_bytes INTEGER, revision INTEGER);
         CREATE TABLE acp_project_usage(project_id TEXT PRIMARY KEY, fact_count INTEGER, payload_bytes INTEGER, revision INTEGER);
         CREATE TABLE acp_store_usage(singleton INTEGER PRIMARY KEY, fact_count INTEGER, fact_bytes INTEGER, shared_payload_bytes INTEGER, revision INTEGER);
         INSERT INTO chats VALUES('chat-one','project-one',0,0);
         INSERT INTO agent_sessions VALUES('session-one','project-one','chat-one','active','hot',0,0,0,NULL);
         INSERT INTO chat_session_heads VALUES('session-one');
         INSERT INTO session_runtime_epochs VALUES('session-one',1,'running',NULL,NULL,0);
         INSERT INTO turns VALUES('turn-one','project-one','chat-one','session-one',1,0,'intent-one','running','correlated',0,0,NULL);
         INSERT INTO acp_session_usage VALUES('session-one',0,0,0,0);
         INSERT INTO acp_project_usage VALUES('project-one',0,0,0);
         INSERT INTO acp_store_usage VALUES(1,0,0,0,0);",
    ).unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/019_turn_workspace_changes.sql"
        ))
        .unwrap();
    connection
}

#[test]
fn restart_limit_plus_one_fails_without_recovering_any_tree() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE agent_sessions(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, state TEXT);
             CREATE TABLE session_runtime_epochs(session_id TEXT, runtime_epoch INTEGER, state TEXT, started_at INTEGER);
             CREATE TABLE turns(id TEXT, session_id TEXT, runtime_epoch INTEGER, state TEXT, delivery_state TEXT, revision INTEGER);",
        )
        .unwrap();
    let trees = sage_core::limits::LIMITS.process.operational_live_trees as i64 + 1;
    for index in 0..trees {
        connection
            .execute_batch(&format!(
                "INSERT INTO agent_sessions VALUES('session-{index}','project-one','chat-{index}','active');
                 INSERT INTO session_runtime_epochs VALUES('session-{index}',1,'running',{index});"
            ))
            .unwrap();
    }
    assert!(matches!(
        recover_after_restart(&mut connection, 50),
        Err(StoreError::Corrupt(
            "live runtime trees exceed the bounded limit"
        ))
    ));
    let live: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM session_runtime_epochs WHERE state='running'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(live, trees);
}

fn add_second_live_tree(connection: &Connection) {
    connection
        .execute_batch(
            "INSERT INTO chats VALUES('chat-two','project-one',0,0);
             INSERT INTO agent_sessions VALUES('session-two','project-one','chat-two','active','hot',0,0,0,NULL);
             INSERT INTO chat_session_heads VALUES('session-two');
             INSERT INTO session_runtime_epochs VALUES('session-two',1,'running',NULL,NULL,1);
             INSERT INTO acp_session_usage VALUES('session-two',0,0,0,0);",
        )
        .unwrap();
}

fn running_trees(connection: &Connection) -> Vec<String> {
    let mut statement = connection
        .prepare("SELECT session_id FROM session_runtime_epochs WHERE state='running' ORDER BY session_id")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn restart_recovers_every_bounded_live_tree() {
    let mut connection = recovery_connection();
    add_second_live_tree(&connection);
    let report = recover_after_restart(&mut connection, 50).unwrap();
    assert_eq!(report.recovered_sessions, 2);
    assert!(running_trees(&connection).is_empty());
}

#[test]
fn zero_process_recovery_leaves_a_concurrent_live_tree_running() {
    let mut connection = recovery_connection();
    add_second_live_tree(&connection);
    let command = RecoverRuntimeCommand {
        project_id: sage_acp_protocol::contract::SageProjectId::parse("project-one").unwrap(),
        chat_id: sage_acp_protocol::contract::SageChatId::parse("chat-two").unwrap(),
        session_id: sage_acp_protocol::contract::SageAgentSessionId::parse("session-two").unwrap(),
        runtime_epoch: sage_acp_protocol::contract::RuntimeEpoch::new(1).unwrap(),
        terminal_reason: TurnTerminalReason::HarnessExited.into(),
        now_millis: 50,
    };
    let report = recover_runtime_after_zero_process(&mut connection, command).unwrap();
    assert_eq!(report.recovered_sessions, 1);
    assert_eq!(running_trees(&connection), ["session-one"]);
}

fn zero_process_command(terminal_reason: TurnTerminalOutcome) -> RecoverRuntimeCommand {
    RecoverRuntimeCommand {
        project_id: sage_acp_protocol::contract::SageProjectId::parse("project-one").unwrap(),
        chat_id: sage_acp_protocol::contract::SageChatId::parse("chat-one").unwrap(),
        session_id: sage_acp_protocol::contract::SageAgentSessionId::parse("session-one").unwrap(),
        runtime_epoch: sage_acp_protocol::contract::RuntimeEpoch::new(1).unwrap(),
        terminal_reason,
        now_millis: 50,
    }
}

fn turn_outcome(connection: &Connection) -> (String, String, Option<String>) {
    connection
        .query_row(
            "SELECT t.state,r.kind,r.message FROM turns t
             JOIN acp_turn_terminal_reasons r ON r.turn_id=t.id
               AND r.session_id=t.session_id AND r.runtime_epoch=t.runtime_epoch
             WHERE t.id='turn-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
}

#[test]
fn a_harness_that_exits_mid_turn_fails_the_turn_with_its_exit_code_and_error() {
    use crate::acp::{process::TreeExit, runtime::terminal_detail::runtime_lost};

    let mut connection = recovery_connection();
    let command = zero_process_command(runtime_lost(
        TreeExit::Exited { code: Some(1) },
        None,
        Some("API Error: 529 overloaded"),
    ));
    recover_runtime_after_zero_process(&mut connection, command).unwrap();
    assert_eq!(
        turn_outcome(&connection),
        (
            "failed".to_owned(),
            "harnessExited".to_owned(),
            Some("harness process exited with code 1: API Error: 529 overloaded".to_owned()),
        )
    );
}

#[test]
fn a_protocol_failure_mid_turn_fails_the_turn_as_a_protocol_error() {
    use crate::acp::{process::TreeExit, runtime::terminal_detail::runtime_lost};

    let mut connection = recovery_connection();
    let command = zero_process_command(runtime_lost(TreeExit::ProtocolFailure, None, None));
    recover_runtime_after_zero_process(&mut connection, command).unwrap();
    assert_eq!(
        turn_outcome(&connection),
        (
            "failed".to_owned(),
            "protocolError".to_owned(),
            Some("harness process stream failed before a clean exit".to_owned()),
        )
    );
}

#[test]
fn replaying_a_zero_process_recovery_keeps_the_first_terminal_reason() {
    let mut connection = recovery_connection();
    let first = TurnTerminalOutcome {
        reason: TurnTerminalReason::HarnessExited,
        message: Some("harness process exited with code 1".to_owned()),
        remediation: None,
    };
    recover_runtime_after_zero_process(&mut connection, zero_process_command(first)).unwrap();
    let replay = recover_runtime_after_zero_process(
        &mut connection,
        zero_process_command(TurnTerminalReason::Timeout.into()),
    )
    .unwrap();
    assert_eq!(replay.recovered_sessions, 0);
    let reasons: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM acp_turn_terminal_reasons",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reasons, 1);
    assert_eq!(turn_outcome(&connection).1, "harnessExited");
}

#[test]
fn a_reason_the_harness_reported_before_the_runtime_was_lost_is_kept() {
    let mut connection = recovery_connection();
    connection
        .execute(
            "INSERT INTO acp_turn_terminal_reasons VALUES
             ('project-one','chat-one','session-one','turn-one',1,'authRequired','Sign in',NULL,40)",
            [],
        )
        .unwrap();
    recover_runtime_after_zero_process(
        &mut connection,
        zero_process_command(TurnTerminalReason::HarnessExited.into()),
    )
    .unwrap();
    assert_eq!(
        turn_outcome(&connection),
        (
            "failed".to_owned(),
            "authRequired".to_owned(),
            Some("Sign in".to_owned())
        )
    );
}

#[test]
fn restart_recovery_says_sage_closed_before_the_turn_finished() {
    let mut connection = recovery_connection();
    recover_after_restart(&mut connection, 50).unwrap();
    assert_eq!(
        turn_outcome(&connection),
        (
            "failed".to_owned(),
            "other".to_owned(),
            Some("Sage closed before this turn finished".to_owned())
        )
    );
}

#[test]
fn a_queued_message_lost_with_its_runtime_is_cancelled_as_never_sent() {
    use crate::acp::{process::TreeExit, runtime::terminal_detail::runtime_lost};

    let mut connection = recovery_connection();
    connection
        .execute(
            "UPDATE turns SET state='queued',delivery_state='notWritten' WHERE id='turn-one'",
            [],
        )
        .unwrap();
    let command = zero_process_command(runtime_lost(
        TreeExit::Exited { code: Some(1) },
        None,
        Some("API Error: 529 overloaded"),
    ));
    recover_runtime_after_zero_process(&mut connection, command).unwrap();
    assert_eq!(
        turn_outcome(&connection),
        (
            "cancelled".to_owned(),
            "other".to_owned(),
            Some("harness process stopped before this message was sent".to_owned()),
        )
    );
}

fn add_error_item(connection: &Connection, ordinal: i64, payload: &str) {
    connection
        .execute(
            "INSERT INTO items VALUES(?1,'session-one',1,'error','completed',NULL,0,0,0,'turn-one',?2,?3)",
            rusqlite::params![format!("error-{ordinal}"), ordinal, payload],
        )
        .unwrap();
}

#[test]
fn a_runtime_loss_after_a_harness_error_keeps_that_error_as_the_message() {
    let mut connection = recovery_connection();
    add_error_item(
        &connection,
        0,
        r#"{"kind":"error","payload":{"code":"stream","message":"stream disconnected before completion","retryable":false}}"#,
    );
    let lost = TurnTerminalOutcome {
        reason: TurnTerminalReason::HarnessExited,
        message: Some("harness process exited with code 1".to_owned()),
        remediation: None,
    };
    recover_runtime_after_zero_process(&mut connection, zero_process_command(lost)).unwrap();
    assert_eq!(
        turn_outcome(&connection),
        ("failed".to_owned(), "harnessExited".to_owned(), None)
    );
}

#[test]
fn a_retry_attempt_error_does_not_hide_the_runtime_loss_message() {
    let mut connection = recovery_connection();
    add_error_item(
        &connection,
        0,
        r#"{"kind":"error","payload":{"kind":"attempt","code":"retry","message":"overloaded, retrying","retryable":true}}"#,
    );
    let lost = TurnTerminalOutcome {
        reason: TurnTerminalReason::HarnessExited,
        message: Some("harness process exited with code 1".to_owned()),
        remediation: None,
    };
    recover_runtime_after_zero_process(&mut connection, zero_process_command(lost)).unwrap();
    assert_eq!(
        turn_outcome(&connection).2.as_deref(),
        Some("harness process exited with code 1")
    );
}

#[test]
fn an_unrecordable_reason_text_never_aborts_recovery() {
    let mut connection = recovery_connection();
    let malformed = TurnTerminalOutcome {
        reason: TurnTerminalReason::Other,
        message: Some("é".repeat(5000)),
        remediation: Some(" \n ".to_owned()),
    };
    let report =
        recover_runtime_after_zero_process(&mut connection, zero_process_command(malformed))
            .unwrap();
    assert_eq!(report.recovered_sessions, 1);
    let (message, remediation): (String, Option<String>) = connection
        .query_row(
            "SELECT message,remediation FROM acp_turn_terminal_reasons WHERE turn_id='turn-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(message, "é".repeat(4096));
    assert_eq!(remediation, None);
}
