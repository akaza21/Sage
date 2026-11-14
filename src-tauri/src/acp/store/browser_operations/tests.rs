use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId},
    projection::BrowserOperationProjection,
};

use super::{
    load,
    write::{finish, recover_started, start},
};

fn setup() -> rusqlite::Connection {
    setup_at(std::path::Path::new(":memory:"))
}

fn open_at(path: &std::path::Path) -> rusqlite::Connection {
    crate::acp::migration::open_or_migrate(path)
        .unwrap()
        .into_parts()
        .0
}

fn setup_at(path: &std::path::Path) -> rusqlite::Connection {
    let connection = open_at(path);
    connection.execute_batch(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Project',1,1,1);
         INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-one','project-one','C:/project','c:/project',0);
         INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Chat',0,1,1);
         INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-one','project-one','chat-one',0,'binding','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','adapter-session','{}','active','hot',0,0,0,1,1);
         INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1);
         INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-one','chat-one','session-one',1,'running','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',1);
         INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-browser','project-one','chat-one','session-one',1,0,'browser-intent','running','completeBytes',0,1,1);
         INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision) VALUES('project-one',1,1,1,0,0,0,0);
         INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,item_bytes,fact_count,fact_bytes,payload_bytes,revision) VALUES('project-one','chat-one','session-one',1,0,0,0,0,0,0);
         UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=1 WHERE singleton=1;",
    ).unwrap();
    connection
}

fn record(number: u32) -> BrowserOperationProjection {
    BrowserOperationProjection {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        turn_id: SageTurnId::parse("turn-browser").unwrap(),
        tab_id: "browser-7".into(),
        operation_id: format!("00000000-0000-4000-8000-{number:012}"),
        operation: "snapshotPage".into(),
        phase: "started".into(),
        origin: "https://example.com".into(),
        summary: "Inspect page".into(),
        result_summary: None,
        created_at: i64::from(number) + 2,
        updated_at: i64::from(number) + 2,
    }
}

#[test]
fn browser_operation_is_durable_and_terminal_update_projects() {
    let mut connection = setup();
    let mut operation = record(1);
    start(&mut connection, RuntimeEpoch::new(1).unwrap(), &operation).unwrap();
    assert!(
        connection
            .execute("UPDATE session_facts SET payload_json=payload_json", [])
            .is_err()
    );
    operation.phase = "completed".into();
    operation.result_summary = Some("Page inspected".into());
    operation.updated_at += 1;
    finish(&mut connection, &operation).unwrap();
    let turn = crate::acp::store::history::TurnRow {
        id: "turn-browser".into(),
        project_id: "project-one".into(),
        chat_id: "chat-one".into(),
        session_id: "session-one".into(),
        session_ordinal: 0,
        runtime_epoch: 1,
        ordinal: 0,
        state: "running".into(),
        revision: 0,
        created_at: 1,
        updated_at: 1,
        terminal_at: None,
        item_before: None,
    };
    let (operations, collapsed) = load(&connection, &turn).unwrap();
    assert_eq!(operations, vec![operation]);
    assert_eq!(collapsed, 0);
}

#[test]
fn fill_value_cannot_enter_durable_browser_operation_projection() {
    let mut connection = setup();
    let mut operation = record(1);
    operation.operation = "act".into();
    operation.summary = "Fill field private-password".into();
    assert!(start(&mut connection, RuntimeEpoch::new(1).unwrap(), &operation).is_err());
    let persisted: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE kind='browserOperation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted, 0);
    operation.summary = "Fill field".into();
    start(&mut connection, RuntimeEpoch::new(1).unwrap(), &operation).unwrap();
    let fact: String = connection
        .query_row(
            "SELECT payload_json FROM session_facts WHERE kind='browserOperation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!fact.contains("private-password"));
    let turn = crate::acp::store::history::TurnRow {
        id: "turn-browser".into(),
        project_id: "project-one".into(),
        chat_id: "chat-one".into(),
        session_id: "session-one".into(),
        session_ordinal: 0,
        runtime_epoch: 1,
        ordinal: 0,
        state: "running".into(),
        revision: 0,
        created_at: 1,
        updated_at: 1,
        terminal_at: None,
        item_before: None,
    };
    let (projected, _) = load(&connection, &turn).unwrap();
    assert_eq!(projected[0].summary, "Fill field");
    assert!(!format!("{projected:?}").contains("private-password"));
}

#[test]
fn sensitive_navigation_path_cannot_enter_durable_projection() {
    let mut connection = setup();
    let mut operation = record(1);
    operation.operation = "navigate".into();
    operation.origin = "https://example.com".into();
    operation.summary = "Navigate to /reset/short-code".into();
    assert!(start(&mut connection, RuntimeEpoch::new(1).unwrap(), &operation).is_err());
    operation.summary = "Navigate to origin".into();
    start(&mut connection, RuntimeEpoch::new(1).unwrap(), &operation).unwrap();
    let fact: String = connection
        .query_row(
            "SELECT payload_json FROM session_facts WHERE kind='browserOperation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!fact.contains("short-code"));
}

#[test]
fn denied_and_unknown_outcomes_project_from_append_only_terminal_facts() {
    let mut connection = setup();
    let mut denied = record(1);
    let mut unknown = record(2);
    start(&mut connection, RuntimeEpoch::new(1).unwrap(), &denied).unwrap();
    start(&mut connection, RuntimeEpoch::new(1).unwrap(), &unknown).unwrap();
    denied.phase = "denied".into();
    denied.result_summary = Some("Browser access denied".into());
    denied.updated_at += 1;
    finish(&mut connection, &denied).unwrap();
    unknown.phase = "outcomeUnknown".into();
    unknown.result_summary = Some("Inspect page before retrying".into());
    unknown.updated_at += 1;
    finish(&mut connection, &unknown).unwrap();
    let turn = crate::acp::store::history::TurnRow {
        id: "turn-browser".into(),
        project_id: "project-one".into(),
        chat_id: "chat-one".into(),
        session_id: "session-one".into(),
        session_ordinal: 0,
        runtime_epoch: 1,
        ordinal: 0,
        state: "running".into(),
        revision: 0,
        created_at: 1,
        updated_at: 1,
        terminal_at: None,
        item_before: None,
    };
    let (operations, collapsed) = load(&connection, &turn).unwrap();
    assert_eq!(operations, vec![denied, unknown]);
    assert_eq!(collapsed, 0);
}

#[test]
fn oldest_operations_collapse_per_turn_and_restart_marks_lingering_start_unknown() {
    let mut connection = setup();
    for number in 1..=34 {
        start(
            &mut connection,
            RuntimeEpoch::new(1).unwrap(),
            &record(number),
        )
        .unwrap();
    }
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE kind='browserOperation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 34);
    let collapsed: i64 = connection
        .query_row(
            "SELECT SUM(json_extract(payload_json,'$.count')) FROM session_facts WHERE kind='browserOperationsCollapsed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(collapsed, 2);
    assert_eq!(recover_started(&mut connection, 100).unwrap(), 34);
    let unknown: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE kind='browserOperationTerminal'
         AND json_extract(payload_json,'$.p')='outcomeUnknown'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unknown, 34);
}

#[test]
fn oldest_chat_operation_collapses_across_turns() {
    let mut connection = setup();
    for number in 1..=129 {
        let mut operation = record(number);
        let turn = (number - 1) / 26;
        if turn != 0 {
            if number % 26 == 1 {
                connection
                    .execute(
                        "UPDATE turns SET state='completed',terminal_at=?1 WHERE state='running'",
                        [i64::from(number)],
                    )
                    .unwrap();
                connection
                    .execute(
                        "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES(?1,'project-one','chat-one','session-one',1,?2,
                     ?3,'running','completeBytes',0,1,1)",
                        rusqlite::params![format!("turn-{turn}"), turn, format!("intent-{turn}")],
                    )
                    .unwrap();
            }
            operation.turn_id = SageTurnId::parse(format!("turn-{turn}")).unwrap();
        }
        start(&mut connection, RuntimeEpoch::new(1).unwrap(), &operation).unwrap();
    }
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE kind='browserOperation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 129);
    let collapsed: i64 = connection
        .query_row(
            "SELECT SUM(json_extract(payload_json,'$.count')) FROM session_facts WHERE kind='browserOperationsCollapsed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(collapsed, 1);
}

#[test]
fn completion_of_an_already_collapsed_operation_does_not_change_its_real_outcome() {
    let mut connection = setup();
    let mut oldest = record(1);
    for number in 1..=33 {
        start(
            &mut connection,
            RuntimeEpoch::new(1).unwrap(),
            &record(number),
        )
        .unwrap();
    }
    oldest.phase = "completed".into();
    oldest.result_summary = Some("Page inspected".into());
    oldest.updated_at = 100;
    assert!(finish(&mut connection, &oldest).is_ok());
}

#[test]
fn terminal_record_fits_at_session_and_global_fact_byte_ceiling() {
    let limit = sage_core::limits::LIMITS.durable.fact_bytes_global as i64;
    for table in ["acp_session_usage", "acp_store_usage"] {
        for recovery in [false, true] {
            let mut connection = setup();
            let mut operation = record(1);
            let reservation = serde_json::to_string(&operation).unwrap().len() as i64
                + super::write::TERMINAL_RESERVE_BYTES as i64;
            let sql = format!("UPDATE {table} SET fact_bytes=?1");
            connection.execute(&sql, [limit - reservation]).unwrap();
            start(&mut connection, RuntimeEpoch::new(1).unwrap(), &operation).unwrap();
            if recovery {
                assert_eq!(recover_started(&mut connection, i64::MAX).unwrap(), 1);
            } else {
                operation.phase = "outcomeUnknown".into();
                operation.result_summary = Some("x".repeat(512));
                operation.updated_at = i64::MAX;
                finish(&mut connection, &operation).unwrap();
            }
            let bytes: i64 = connection
                .query_row(&format!("SELECT fact_bytes FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert!(bytes <= limit, "{table} exceeded its fact byte ceiling");
        }
    }
}

#[test]
fn restart_recovers_more_than_one_batch_of_unterminated_operations() {
    let mut connection = setup();
    let mut start_bytes = 0_i64;
    {
        let tx = connection.transaction().unwrap();
        let mut insert = tx.prepare_cached(
            "INSERT INTO session_facts(project_id,chat_id,session_id,sequence,turn_id,kind,payload_json,payload_bytes,payload_digest,projection_revision,created_at)
             VALUES('project-one','chat-one','session-one',?1,'turn-browser','browserOperation',?2,?3,?4,1,?5)",
        ).unwrap();
        for number in 1..=8_193 {
            let operation = record(number);
            let payload = serde_json::to_string(&operation).unwrap();
            let bytes = payload.len() as i64;
            insert
                .execute(rusqlite::params![
                    number,
                    payload,
                    bytes,
                    super::super::facts::hex_digest(payload.as_bytes()),
                    operation.created_at
                ])
                .unwrap();
            start_bytes += bytes;
        }
        drop(insert);
        let reserved_bytes = start_bytes + 8_193 * super::write::TERMINAL_RESERVE_BYTES as i64;
        tx.execute(
            "UPDATE acp_session_usage SET fact_count=16386,fact_bytes=?1,payload_bytes=?1",
            [reserved_bytes],
        )
        .unwrap();
        tx.execute(
            "UPDATE acp_project_usage SET fact_count=16386,payload_bytes=?1",
            [reserved_bytes],
        )
        .unwrap();
        tx.execute(
            "UPDATE acp_store_usage SET fact_count=16386,fact_bytes=?1,shared_payload_bytes=?1",
            [reserved_bytes],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    assert_eq!(recover_started(&mut connection, 100_000).unwrap(), 8_193);
    let recovered: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE kind='browserOperationTerminal'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(recovered, 8_193);
    assert_eq!(recover_started(&mut connection, 100_001).unwrap(), 0);
}

fn turn_row(id: &str) -> crate::acp::store::history::TurnRow {
    crate::acp::store::history::TurnRow {
        id: id.into(),
        project_id: "project-one".into(),
        chat_id: "chat-one".into(),
        session_id: "session-one".into(),
        session_ordinal: 0,
        runtime_epoch: 1,
        ordinal: 0,
        state: "running".into(),
        revision: 0,
        created_at: 1,
        updated_at: 1,
        terminal_at: None,
        item_before: None,
    }
}

fn begin_turn(connection: &rusqlite::Connection, ordinal: i64) {
    connection
        .execute(
            "UPDATE turns SET state='completed',terminal_at=1 WHERE state='running'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at)
             VALUES(?1,'project-one','chat-one','session-one',1,?2,?3,'running','completeBytes',0,1,1)",
            rusqlite::params![format!("turn-{ordinal}"), ordinal, format!("intent-{ordinal}")],
        )
        .unwrap();
}

#[test]
fn each_turn_projects_only_its_own_operations_in_order() {
    let mut connection = setup();
    let epoch = RuntimeEpoch::new(1).unwrap();
    let mut started = Vec::new();
    for (number, turn) in [
        (1, None),
        (2, None),
        (3, Some(1)),
        (4, Some(1)),
        (5, Some(2)),
    ] {
        if number == 3 || number == 5 {
            begin_turn(&connection, turn.unwrap());
        }
        let mut operation = record(number);
        if let Some(turn) = turn {
            operation.turn_id = SageTurnId::parse(format!("turn-{turn}")).unwrap();
        }
        start(&mut connection, epoch, &operation).unwrap();
        started.push(operation);
    }
    // Terminals land after later Turns' facts; each still joins its own start.
    for (index, phase) in [(0, "completed"), (3, "failed"), (1, "denied")] {
        let operation = &mut started[index];
        operation.phase = phase.into();
        operation.result_summary = Some(format!("{phase} {index}"));
        operation.updated_at += 10;
        finish(&mut connection, operation).unwrap();
    }

    for (turn, expected) in [
        ("turn-browser", &started[0..2]),
        ("turn-1", &started[2..4]),
        ("turn-2", &started[4..5]),
    ] {
        let (operations, collapsed) = load(&connection, &turn_row(turn)).unwrap();
        assert_eq!(operations, expected, "{turn}");
        assert_eq!(collapsed, 0);
    }
    assert_eq!(load(&connection, &turn_row("turn-9")).unwrap().0, vec![]);
}

fn start_in_turn(
    connection: &mut rusqlite::Connection,
    numbers: std::ops::RangeInclusive<u32>,
    turn: &str,
) -> Vec<BrowserOperationProjection> {
    numbers
        .map(|number| {
            let mut operation = record(number);
            operation.turn_id = SageTurnId::parse(turn).unwrap();
            start(connection, RuntimeEpoch::new(1).unwrap(), &operation).unwrap();
            operation
        })
        .collect()
}

#[test]
fn turn_page_hides_operations_collapsed_past_the_turn_cap() {
    let mut connection = setup();
    let earlier = start_in_turn(&mut connection, 1..=2, "turn-browser");
    begin_turn(&connection, 1);
    // Two past the per-Turn cap of 32, so the Turn's two oldest collapse.
    let crowded = start_in_turn(&mut connection, 3..=36, "turn-1");

    assert_eq!(
        load(&connection, &turn_row("turn-1")).unwrap(),
        (crowded[2..].to_vec(), 2)
    );
    assert_eq!(
        load(&connection, &turn_row("turn-browser")).unwrap(),
        (earlier, 0)
    );
}

#[test]
fn turn_page_hides_operations_collapsed_past_the_chat_cap() {
    let mut connection = setup();
    let mut turns = vec![(
        "turn-browser".to_owned(),
        start_in_turn(&mut connection, 1..=4, "turn-browser"),
    )];
    // Five Turns of 25 stay under the per-Turn cap and bring the chat to 129
    // operations, one past its cap of 128, so only the chat's oldest operation
    // collapses, out of an earlier Turn.
    for ordinal in 1_u32..=5 {
        begin_turn(&connection, i64::from(ordinal));
        let turn = format!("turn-{ordinal}");
        let first = 5 + (ordinal - 1) * 25;
        let operations = start_in_turn(&mut connection, first..=first + 24, &turn);
        turns.push((turn, operations));
    }
    turns[0].1.remove(0);

    for (index, (turn, operations)) in turns.into_iter().enumerate() {
        let collapsed = u64::from(index == 0);
        assert_eq!(
            load(&connection, &turn_row(&turn)).unwrap(),
            (operations, collapsed),
            "{turn}"
        );
    }
}

fn plan(connection: &rusqlite::Connection, sql: &str, values: &[&dyn rusqlite::ToSql]) -> String {
    connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap()
        .query_map(values, |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n")
}

#[test]
fn fact_lookups_seek_their_index() {
    const TURN: &str = "idx_session_facts_turn";
    const TERMINAL: &str = "idx_session_facts_operation_terminal";
    const START: &str = "idx_session_facts_operation_start";
    let connection = setup();
    let owner: [&dyn rusqlite::ToSql; 4] = [&"project-one", &"chat-one", &"session-one", &"turn"];
    let cases = [
        (
            plan(&connection, super::TURN_OPERATIONS_SQL, &owner),
            [("start", TURN), ("marker", TURN)].as_slice(),
        ),
        (
            plan(&connection, super::TERMINAL_SQL, &[&"session-one", &1_i64]),
            [("session_facts", TERMINAL)].as_slice(),
        ),
        (
            plan(
                &connection,
                crate::acp::store::turn_activity_facts::TURN_FACTS_SQL,
                &[&"session-one", &"turn", &"sessionActivity", &80_i64],
            ),
            [("session_facts", TURN)].as_slice(),
        ),
        (
            plan(
                &connection,
                super::write::RECOVER_SQL,
                &[&"session-one", &0_i64],
            ),
            [("start", START), ("terminal", TERMINAL)].as_slice(),
        ),
    ];
    for (plan, seeks) in cases {
        for (table, index) in seeks {
            assert!(
                plan.lines().any(|step| {
                    step.split_whitespace().nth(1) == Some(table)
                        && step.contains(index)
                        // A terminal is sought by its `$.s` key, not filtered after.
                        && (*index != TERMINAL || step.contains("<expr>=?"))
                }),
                "{table} does not seek {index}:\n{plan}"
            );
        }
        assert!(!plan.contains("SCAN"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
        assert!(!plan.contains("MATERIALIZE"), "{plan}");
    }
}

#[test]
fn upgraded_store_with_a_duplicate_terminal_recovers_only_the_unfinished_operation() {
    let directory = std::env::temp_dir().join(format!(
        "sage-browser-terminal-upgrade-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let mut finished = record(1);
    let unfinished = record(2);
    {
        let mut connection = setup_at(&path);
        for operation in [&finished, &unfinished] {
            start(&mut connection, RuntimeEpoch::new(1).unwrap(), operation).unwrap();
        }
        finished.phase = "completed".into();
        finished.result_summary = Some("Page inspected".into());
        finished.updated_at += 1;
        finish(&mut connection, &finished).unwrap();
        // Return the file to schema 26, before the operation indexes, with its
        // facts already written and a duplicate terminal an older store could
        // hold, so the next open has to upgrade it.
        let identity_trigger = crate::acp::migration::historical_trigger_sql(
            26,
            "deny_agent_session_identity_mutation",
        )
        .unwrap();
        connection
            .execute_batch(&format!(
                "DROP TRIGGER deny_agent_session_identity_mutation;
                 {identity_trigger};
                 INSERT INTO session_facts(project_id,chat_id,session_id,sequence,turn_id,kind,
                   payload_json,payload_bytes,payload_digest,projection_revision,created_at)
                 SELECT project_id,chat_id,session_id,sequence+1,turn_id,kind,
                   payload_json,payload_bytes,payload_digest,projection_revision,created_at
                 FROM session_facts WHERE kind='browserOperationTerminal';
                 DROP INDEX idx_session_facts_operation_start;
                 DROP INDEX idx_session_facts_operation_terminal;
                 DROP INDEX idx_session_facts_undo_start;
                 DROP INDEX idx_session_facts_undo_recovery;
                 DROP INDEX idx_session_facts_undo_settled;
                 DROP TABLE acp_imported_histories;
                 DROP INDEX idx_agent_sessions_external_session;
                 DELETE FROM schema_migrations WHERE version>=27;
                 PRAGMA user_version=26;",
            ))
            .unwrap();
    }

    let mut connection = open_at(&path);
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, crate::acp::migration::CURRENT_SCHEMA_VERSION);
    assert_eq!(recover_started(&mut connection, 100).unwrap(), 1);
    assert_eq!(recover_started(&mut connection, 101).unwrap(), 0);
    let mut unknown = unfinished;
    unknown.phase = "outcomeUnknown".into();
    unknown.result_summary = Some("Sage restarted before the browser result was known".into());
    unknown.updated_at = 100;
    assert_eq!(
        load(&connection, &turn_row("turn-browser")).unwrap(),
        (vec![finished, unknown], 0)
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}
