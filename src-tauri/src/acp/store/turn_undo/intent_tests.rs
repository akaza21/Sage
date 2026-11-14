use super::*;

fn assert_fact_seeks(
    connection: &Connection,
    sql: &str,
    values: &[&dyn rusqlite::ToSql],
    seeks: &[(&str, &str, &str)],
) {
    let plan = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap()
        .query_map(values, |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    for (alias, index, key) in seeks {
        assert!(
            plan.lines().any(|step| {
                step.starts_with(&format!("SEARCH {alias} USING INDEX {index} "))
                    && step.contains(key)
            }),
            "{alias} must seek {index} by {key}:\n{plan}"
        );
    }
    assert!(!plan.contains("TEMP B-TREE"), "{plan}");
}

#[test]
fn upgrading_a_populated_ledger_seeks_undo_intents_and_recovers_only_the_unsettled_start() {
    let fixture = fixture::Fixture::new();
    fixture.run();
    fixture.turn("turn-1", 0);
    let command = TurnUndoneCommand {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
        turn_id: SageTurnId::parse("turn-1").unwrap(),
        harness_reverted: false,
        cut_at: None,
        now_millis: 50,
    };
    {
        let mut connection = fixture.database.connection();
        // Persist an old schema with ordinary history, a settled attempt and
        // a second attempt on the same Turn. Its receipt must name its start.
        connection
            .execute_batch(
                "DROP INDEX idx_session_facts_undo_start;
             DROP INDEX idx_session_facts_undo_recovery;
             DROP INDEX idx_session_facts_undo_settled;
             DROP TABLE acp_imported_histories;
             DROP INDEX idx_agent_sessions_external_session;
             DELETE FROM schema_migrations WHERE version>=28;
             PRAGMA user_version=27;",
            )
            .unwrap();
        let tx = connection.transaction().unwrap();
        for _ in 0..2048 {
            append_fact_payload(
                &tx,
                fixture.project.as_str(),
                fixture.chat.as_str(),
                "session-a",
                Some("turn-1"),
                "sendIntent",
                "{}",
                ProjectionRevision::new(1).unwrap(),
                20,
            )
            .unwrap();
        }
        tx.commit().unwrap();
        intent::ensure_settled(&connection, fixture.project.as_str(), fixture.chat.as_str())
            .unwrap();
        intent::start(&mut connection, &command).unwrap();
        intent::rollback(&mut connection, &command).unwrap();
        intent::start(&mut connection, &command).unwrap();
        super::super::park_session_after_zero_process(
            &mut connection,
            super::super::ParkSessionCommand {
                project_id: fixture.project.clone(),
                chat_id: fixture.chat.clone(),
                session_id: command.session_id.clone(),
                runtime_epoch: command.runtime_epoch,
                exit_kind: sage_acp_protocol::contract::RequiredText::new("exitKind", "shutdown")
                    .unwrap(),
                now_millis: 60,
            },
        )
        .unwrap();
    }
    let reopened = crate::database::open_database(&fixture.root.join("sage.db")).unwrap();
    let mut connection = reopened.connection();
    connection.execute_batch("ANALYZE").unwrap();
    assert!(matches!(
        intent::ensure_settled(&connection, fixture.project.as_str(), fixture.chat.as_str()),
        Err(StoreError::Conflict {
            resource: "unsettledUndo"
        })
    ));
    assert_fact_seeks(
        &connection,
        intent::PENDING_SQL,
        &[&fixture.project.as_str(), &fixture.chat.as_str()],
        &[
            (
                "s",
                "idx_session_facts_undo_start",
                "project_id=? AND chat_id=?",
            ),
            ("f", "idx_session_facts_undo_settled", "<expr>=?"),
        ],
    );
    assert_fact_seeks(
        &connection,
        recovery::RECOVER_PARKED_SQL,
        &[],
        &[
            ("f", "idx_session_facts_undo_recovery", "kind=?"),
            ("s", "idx_session_facts_undo_settled", "<expr>=?"),
        ],
    );
    assert_eq!(recovery::recover_parked(&mut connection, 70).unwrap().0, 1);
    assert_eq!(recovery::recover_parked(&mut connection, 80).unwrap().0, 0);
    intent::ensure_settled(&connection, fixture.project.as_str(), fixture.chat.as_str()).unwrap();
    assert_eq!(fixture.page_turns(), ["turn-1"]);
    let page = super::super::load_chat_page(
        &connection,
        fixture.project.clone(),
        fixture.chat.clone(),
        None,
    )
    .unwrap();
    assert!(page.current_session_id.is_none());
}
