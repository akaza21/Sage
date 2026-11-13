use super::*;

pub(super) const V23: &str = include_str!("../../../migrations/023_session_config_choices.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentV23
        | SchemaClass::CurrentResumeLink
        | SchemaClass::CurrentTitleSource
        | SchemaClass::CurrentSessionFactsTurnIndex
        | SchemaClass::CurrentOperationTerminalIndex
        | SchemaClass::CurrentTurnUndoIntentIndexes => return Ok(()),
        SchemaClass::CurrentV22 => {}
        _ => return invalid("session config choices require exact schema v22"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V23)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (23,?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 23)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV23 {
        return invalid("database did not reach exact schema v23");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v22() -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        title_provenance::apply(&mut connection).unwrap();
        session_scope::apply(&mut connection).unwrap();
        notifications::apply(&mut connection).unwrap();
        connection
    }

    #[test]
    fn upgrade_is_exact_idempotent_and_cascades_with_the_session() {
        let mut connection = v22();
        connection.execute_batch("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
            INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat',1,1);").unwrap();
        connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,state,residency,created_at,updated_at) VALUES('session','project','chat',0,'binding',?1,'{}','created','cold',1,1)", [&"a".repeat(64)]).unwrap();
        apply(&mut connection).unwrap();
        let first = fingerprint::database_fingerprint(&connection).unwrap();
        assert_eq!(first.schema, SchemaClass::CurrentV23);
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::database_fingerprint(&connection).unwrap(),
            first
        );
        connection
            .execute("INSERT INTO acp_session_config_choices VALUES('session','option','effort','{\"value\":\"high\"}',1)", [])
            .unwrap();
        connection
            .execute("INSERT INTO acp_session_config_notices VALUES('session','model','retired','current','notOffered',1)", [])
            .unwrap();
        assert!(
            connection
                .execute("UPDATE acp_session_config_notices SET reason='other'", [])
                .is_err()
        );
        // Oversized values and unknown kinds are refused by the schema itself.
        assert!(
            connection
                .execute(
                    "INSERT INTO acp_session_config_choices VALUES('session','other','x','{}',1)",
                    []
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO acp_session_config_choices VALUES('session','option','big',?1,1)",
                    [format!("{{\"value\":\"{}\"}}", "x".repeat(8192))],
                )
                .is_err()
        );
        // Temporary chat cleanup lifts the history guard, then deletes the
        // session; its choices go with it.
        connection
            .execute_batch(
                "DROP TRIGGER deny_agent_session_delete;
                 DELETE FROM agent_sessions WHERE id='session';",
            )
            .unwrap();
        for table in ["acp_session_config_choices", "acp_session_config_notices"] {
            let left: i64 = connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(left, 0, "{table}");
        }
    }

    #[test]
    fn upgrade_refuses_a_conflicting_table_and_leaves_v22() {
        let mut connection = v22();
        connection
            .execute_batch("CREATE TABLE acp_session_config_choices(unrelated TEXT);")
            .unwrap();
        assert!(apply(&mut connection).is_err());
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 22);
    }
}
