use super::*;

pub(super) const V19: &str = include_str!("../../../migrations/019_turn_workspace_changes.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    if matches!(
        fingerprint::classify_connection(connection)?,
        SchemaClass::CurrentV19
            | SchemaClass::CurrentV20
            | SchemaClass::CurrentV21
            | SchemaClass::CurrentV22
            | SchemaClass::CurrentResumeLink
    ) {
        return Ok(());
    }
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV18 {
        return invalid("workspace summaries require exact schema v18");
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V19)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (19,?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 19)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV19 {
        return invalid("database did not reach exact schema v19");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v18_workspace_upgrade_is_exact_idempotent_and_preserves_existing_rows() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Preserved',1,1,1)", []).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV19
        );
        assert_eq!(
            connection
                .query_row("SELECT name FROM projects", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "Preserved"
        );
        let first = fingerprint::database_fingerprint(&connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::database_fingerprint(&connection).unwrap(),
            first
        );
    }

    #[test]
    fn partial_workspace_schema_and_downgraded_markers_fail_closed() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        let before = fingerprint::database_fingerprint(&connection).unwrap();
        {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(V19).unwrap();
        }
        assert_eq!(
            fingerprint::database_fingerprint(&connection).unwrap(),
            before
        );
        apply(&mut connection).unwrap();
        connection
            .execute("DELETE FROM schema_migrations WHERE version=19", [])
            .unwrap();
        connection.pragma_update(None, "user_version", 18).unwrap();
        assert!(fingerprint::classify_connection(&connection).is_err());
        assert!(apply(&mut connection).is_err());
    }

    #[test]
    fn populated_v18_sessions_turns_transcripts_and_usage_survive_upgrade_and_reopen() {
        let root =
            std::env::temp_dir().join(format!("sage-workspace-upgrade-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sage.db");
        let mut source = Connection::open(&path).unwrap();
        schema_upgrade::build_fresh_v18(&mut source).unwrap();
        source.execute_batch("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Preserved',1,1,1);
            INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat',1,1);
            INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,state,residency,created_at,updated_at) VALUES('session','project','chat',0,'binding','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','{}','ready','hot',1,1);
            INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project','chat','session',1,'running','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',1);
            INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,created_at,updated_at,terminal_at) VALUES('turn','project','chat','session',1,0,'intent','completed','correlated',1,2,2);
            INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,created_at,updated_at,terminal_at) VALUES('item','project','chat','session','turn',1,0,'primary','text','completed','{\"kind\":\"text\",\"payload\":{\"text\":\"prompt\"}}',43,'b2bf66ef982a206f5552f31203f5825ec8155829efc5a4d0f63ba817a6dbe9fe',1,2,2);
            INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,item_bytes,fact_count,fact_bytes,payload_bytes,revision) VALUES('project','chat','session',1,1,43,0,0,43,0);
            INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision) VALUES('project',1,1,1,1,0,43,0);
            UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=1,item_count=1,item_bytes=43,shared_payload_bytes=43 WHERE singleton=1;").unwrap();
        drop(source);
        for _attempt in 0..2 {
            let opened = open_or_migrate(&path).unwrap();
            assert_eq!(
                fingerprint::classify_connection(&opened).unwrap(),
                crate::acp::migration::fingerprint::CURRENT_SCHEMA
            );
            let row: (String,String,String,i64,i64,i64) = opened.query_row("SELECT s.state,t.state,i.payload_json,u.payload_bytes,p.payload_bytes,g.shared_payload_bytes FROM agent_sessions s JOIN turns t ON t.session_id=s.id JOIN items i ON i.turn_id=t.id JOIN acp_session_usage u ON u.session_id=s.id JOIN acp_project_usage p ON p.project_id=s.project_id CROSS JOIN acp_store_usage g",[],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).unwrap();
            assert_eq!(
                row,
                (
                    "ready".to_owned(),
                    "completed".to_owned(),
                    "{\"kind\":\"text\",\"payload\":{\"text\":\"prompt\"}}".to_owned(),
                    43,
                    43,
                    43
                )
            );
            assert_eq!(
                opened
                    .query_row(
                        "SELECT COUNT(*) FROM acp_turn_workspace_changes",
                        [],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
