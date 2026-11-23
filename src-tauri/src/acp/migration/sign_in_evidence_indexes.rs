use super::*;

pub(super) const VERSION: u32 = 33;
pub(super) const SQL: &str = include_str!("../../../migrations/033_sign_in_evidence_indexes.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentSignInEvidenceIndexes
        | SchemaClass::CurrentAgentFileWrites
        | SchemaClass::CurrentCatalogCapabilities => {
            return Ok(());
        }
        SchemaClass::CurrentPrivateProjects => {}
        _ => return invalid("sign-in evidence indexes require the exact private projects schema"),
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .db()?;
    transaction.execute_batch(SQL).db()?;
    transaction
        .execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES (?1,?2)",
            rusqlite::params![VERSION, now_millis()],
        )
        .db()?;
    transaction
        .pragma_update(None, "user_version", VERSION)
        .db()?;
    transaction.commit().db()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentSignInEvidenceIndexes {
        return invalid("database did not reach the exact sign-in evidence index schema");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::store::commands::sign_in_evidence::{RefusalCommand, command_terminal_at};
    use rusqlite::{params, types::Value};

    fn retained_rows(connection: &Connection) -> Vec<Vec<Vec<Value>>> {
        [
            "SELECT * FROM projects ORDER BY id",
            "SELECT * FROM chats ORDER BY id",
            "SELECT * FROM agent_sessions ORDER BY id",
            "SELECT * FROM session_facts ORDER BY session_id,sequence",
            "SELECT * FROM acp_session_usage ORDER BY session_id",
            "SELECT * FROM acp_project_usage ORDER BY project_id",
            "SELECT * FROM acp_store_usage ORDER BY singleton",
        ]
        .map(|sql| {
            let mut statement = connection.prepare(sql).unwrap();
            let columns = statement.column_count();
            statement
                .query_map([], |row| {
                    (0..columns).map(|column| row.get(column)).collect()
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        })
        .into()
    }

    fn refusal() -> RefusalCommand<'static> {
        RefusalCommand {
            project_id: "project",
            chat_id: "chat",
            client_intent_id: "config",
            session_id: Some("session"),
        }
    }

    fn seed_history(connection: &mut Connection) {
        connection
            .execute_batch(
                "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
             VALUES('project','Project',1,1,1);
             INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
             VALUES('chat','project',0,'Chat',1,1,1);",
            )
            .unwrap();
        connection.execute(
            "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
               external_session_id,capabilities_json,state,residency,projection_revision,created_at,updated_at)
             VALUES('session','project','chat',0,'binding',?1,'external','{}','ready','hot',1,1,1)",
            ["a".repeat(64)],
        ).unwrap();
        let facts = [
            ("sessionStartIntent", r#"{"clientIntentId":"start"}"#),
            ("adapterBatch", r#"{"text":"Keep this transcript"}"#),
        ];
        for (index, (kind, payload)) in facts.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO session_facts(project_id,chat_id,session_id,sequence,kind,
                   payload_json,payload_bytes,payload_digest,projection_revision,created_at)
                 VALUES('project','chat','session',?1,?2,?3,?4,?5,1,10)",
                    params![
                        index as i64 + 1,
                        kind,
                        payload,
                        payload.len() as i64,
                        crate::acp::store::hex_digest(payload.as_bytes())
                    ],
                )
                .unwrap();
        }
        let bytes = facts
            .iter()
            .map(|(_, payload)| payload.len() as i64)
            .sum::<i64>();
        connection
            .execute(
                "INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,
               item_count,fact_count,payload_bytes,revision) VALUES('project',1,1,0,0,2,?1,2)",
                [bytes],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,
               item_bytes,fact_count,fact_bytes,payload_bytes,revision)
             VALUES('project','chat','session',0,0,0,2,?1,?1,2)",
                [bytes],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE acp_store_usage SET chat_count=1,session_count=1,fact_count=2,
               fact_bytes=?1,shared_payload_bytes=?1,revision=2 WHERE singleton=1",
                [bytes],
            )
            .unwrap();
        assert_eq!(
            command_terminal_at(connection, refusal(), 200).unwrap(),
            200
        );
        for table in ["acp_session_usage", "acp_project_usage", "acp_store_usage"] {
            let count: i64 = connection
                .query_row(&format!("SELECT fact_count FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 3);
        }
    }

    #[test]
    fn upgrades_private_projects_and_reopens_without_changing_history() {
        let directory = std::env::current_dir()
            .unwrap()
            .join("work")
            .join(format!("sign-in-index-migration-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        let mut connection = Connection::open(&path).unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        title_provenance::apply(&mut connection).unwrap();
        session_scope::apply(&mut connection).unwrap();
        notifications::apply(&mut connection).unwrap();
        config_choices::apply(&mut connection).unwrap();
        session_resume_link::apply(&mut connection).unwrap();
        title_source::apply(&mut connection).unwrap();
        session_facts_turn_index::apply(&mut connection).unwrap();
        operation_terminal_index::apply(&mut connection).unwrap();
        turn_undo_intent_indexes::apply(&mut connection).unwrap();
        imported_history::apply(&mut connection).unwrap();
        session_capabilities::apply(&mut connection).unwrap();
        child_thread_states::apply(&mut connection).unwrap();
        private_projects::apply(&mut connection).unwrap();
        seed_history(&mut connection);
        let before = retained_rows(&connection);
        apply(&mut connection).unwrap();
        assert_eq!(retained_rows(&connection), before);
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentSignInEvidenceIndexes
        );
        agent_file_writes::apply(&mut connection).unwrap();
        catalog_capabilities::apply(&mut connection).unwrap();
        assert_eq!(retained_rows(&connection), before);
        let migrated = database_fingerprint(&connection).unwrap();
        post_v18::apply(&mut connection).unwrap();
        assert_eq!(database_fingerprint(&connection).unwrap(), migrated);
        assert_eq!(migrated.schema, fingerprint::CURRENT_SCHEMA);
        drop(connection);

        let (mut reopened, anchor) = open_or_migrate(&path).unwrap().into_parts();
        assert_eq!(retained_rows(&reopened), before);
        assert_eq!(
            command_terminal_at(&mut reopened, refusal(), 999).unwrap(),
            200
        );
        assert_eq!(retained_rows(&reopened), before);
        assert_eq!(database_fingerprint(&reopened).unwrap(), migrated);
        drop(reopened);
        drop(anchor);
        fs::remove_dir_all(directory).unwrap();
    }
}
