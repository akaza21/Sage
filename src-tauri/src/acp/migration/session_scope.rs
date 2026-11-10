use super::*;

pub(super) const V21: &str = include_str!("../../../migrations/021_acp_session_scope.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentV21
        | SchemaClass::CurrentV22
        | SchemaClass::CurrentV23
        | SchemaClass::CurrentResumeLink
        | SchemaClass::CurrentTitleSource
        | SchemaClass::CurrentSessionFactsTurnIndex => {
            return Ok(());
        }
        SchemaClass::CurrentV20 => {}
        _ => return invalid("session surfaces require exact schema v20"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V21)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (21,?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 21)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV21 {
        return invalid("database did not reach exact schema v21");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_is_exact_idempotent_and_requires_v20() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        title_provenance::apply(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
                 INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                   VALUES('chat','project',0,'Chat',5,1,1);",
            )
            .unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(intent_revision(&connection, "chat"), 5);
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV21
        );
        let first = fingerprint::database_fingerprint(&connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::database_fingerprint(&connection).unwrap(),
            first
        );
        connection
            .execute_batch(
                "DROP TABLE acp_session_surfaces;
                 DELETE FROM schema_migrations WHERE version IN (20,21);
                 PRAGMA user_version=19;",
            )
            .unwrap();
        assert!(apply(&mut connection).is_err());
    }

    #[test]
    fn every_projection_change_advances_the_intent_revision() {
        let connection = open_or_migrate(Path::new(":memory:"))
            .unwrap()
            .into_parts()
            .0;
        connection
            .execute_batch(
                "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
                 INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                   VALUES('chat','project',0,'Chat',3,1,1);",
            )
            .unwrap();
        assert_eq!(intent_revision(&connection, "chat"), 3);
        connection
            .execute("UPDATE chats SET projection_revision=4 WHERE id='chat'", [])
            .unwrap();
        assert_eq!(intent_revision(&connection, "chat"), 4);
    }

    fn intent_revision(connection: &Connection, chat: &str) -> i64 {
        connection
            .query_row(
                "SELECT intent_revision FROM chats WHERE id=?1",
                [chat],
                |row| row.get(0),
            )
            .unwrap()
    }
}
