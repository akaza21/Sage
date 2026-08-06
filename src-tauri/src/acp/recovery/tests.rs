use super::*;
use rusqlite::Connection;

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
            TurnState::Interrupted,
        ),
        (
            (TurnState::WaitingTool, DeliveryState::Correlated),
            TurnState::Interrupted,
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
         CREATE TABLE turns(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, session_id TEXT, runtime_epoch INTEGER, state TEXT, delivery_state TEXT, revision INTEGER, updated_at INTEGER, terminal_at INTEGER);
         CREATE TABLE items(id TEXT PRIMARY KEY,session_id TEXT,runtime_epoch INTEGER,kind TEXT,lifecycle TEXT,tool_state TEXT,revision INTEGER,updated_at INTEGER,terminal_at INTEGER);
         CREATE TABLE session_activities(id TEXT PRIMARY KEY,session_id TEXT,runtime_epoch INTEGER,lifecycle TEXT,revision INTEGER,updated_at INTEGER,terminal_at INTEGER);
         CREATE TABLE server_requests(id TEXT PRIMARY KEY,session_id TEXT,runtime_epoch INTEGER,state TEXT,revision INTEGER,updated_at INTEGER,resolved_at INTEGER);
         CREATE TABLE session_facts(project_id TEXT, chat_id TEXT, session_id TEXT, sequence INTEGER, turn_id TEXT, kind TEXT, payload_json TEXT, payload_bytes INTEGER, payload_digest TEXT, projection_revision INTEGER, created_at INTEGER, PRIMARY KEY(session_id,sequence));
         CREATE TABLE acp_session_usage(session_id TEXT PRIMARY KEY, fact_count INTEGER, fact_bytes INTEGER, payload_bytes INTEGER, revision INTEGER);
         CREATE TABLE acp_project_usage(project_id TEXT PRIMARY KEY, fact_count INTEGER, payload_bytes INTEGER, revision INTEGER);
         CREATE TABLE acp_store_usage(singleton INTEGER PRIMARY KEY, fact_count INTEGER, fact_bytes INTEGER, shared_payload_bytes INTEGER, revision INTEGER);
         INSERT INTO chats VALUES('chat-one','project-one',0,0);
         INSERT INTO agent_sessions VALUES('session-one','project-one','chat-one','active','hot',0,0,0,NULL);
         INSERT INTO chat_session_heads VALUES('session-one');
         INSERT INTO session_runtime_epochs VALUES('session-one',1,'running',NULL,NULL,0);
         INSERT INTO turns VALUES('turn-one','project-one','chat-one','session-one',1,'sending','writing',0,0,NULL);
         INSERT INTO items VALUES('item-one','session-one',1,'permissionRequest','streaming','waitingForApproval',0,0,NULL);
         INSERT INTO session_activities VALUES('activity-one','session-one',1,'active',0,0,NULL);
         INSERT INTO server_requests VALUES('request-one','session-one',1,'received',0,0,NULL);
         INSERT INTO acp_session_usage VALUES('session-one',0,0,0,0);
         INSERT INTO acp_project_usage VALUES('project-one',0,0,0);
         INSERT INTO acp_store_usage VALUES(1,0,0,0,0);",
    ).unwrap();

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
fn restart_limit_plus_one_fails_without_recovering_either_tree() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(
        "CREATE TABLE agent_sessions(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, state TEXT);
         CREATE TABLE session_runtime_epochs(session_id TEXT, runtime_epoch INTEGER, state TEXT, started_at INTEGER);
         CREATE TABLE turns(id TEXT, session_id TEXT, runtime_epoch INTEGER, state TEXT, delivery_state TEXT, revision INTEGER);
         INSERT INTO agent_sessions VALUES('session-one','project-one','chat-one','active');
         INSERT INTO agent_sessions VALUES('session-two','project-one','chat-two','active');
         INSERT INTO session_runtime_epochs VALUES('session-one',1,'running',0);
         INSERT INTO session_runtime_epochs VALUES('session-two',1,'running',1);",
    ).unwrap();
    assert!(matches!(
        recover_after_restart(&mut connection, 50),
        Err(StoreError::Corrupt("multiple live runtime trees"))
    ));
    let live: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM session_runtime_epochs WHERE state='running'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(live, 2);
}
