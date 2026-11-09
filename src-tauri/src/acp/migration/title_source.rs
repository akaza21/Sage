use super::*;

/// The schema version that records each chat title's source. The migration's
/// file name carries the same number; nothing else names it.
pub(super) const VERSION: u32 = 25;
pub(super) const SQL: &str = include_str!("../../../migrations/025_chat_title_source.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentTitleSource => return Ok(()),
        SchemaClass::CurrentResumeLink => {}
        _ => return invalid("chat title sources require the exact session resume link schema"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(SQL)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (?1,?2)",
        rusqlite::params![VERSION, now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", VERSION)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentTitleSource {
        return invalid("database did not reach the exact chat title source schema");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resume_link_with_chats() -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        title_provenance::apply(&mut connection).unwrap();
        session_scope::apply(&mut connection).unwrap();
        notifications::apply(&mut connection).unwrap();
        config_choices::apply(&mut connection).unwrap();
        session_resume_link::apply(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
                 INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at,title_is_default)
                   VALUES('placeholder','project',0,'Chat: Chat',1,1,1),
                         ('named','project',1,'Chat: Release notes',1,1,0);",
            )
            .unwrap();
        connection
    }

    fn source(connection: &Connection, chat: &str) -> String {
        connection
            .query_row(
                "SELECT title_source FROM chats WHERE id=?1",
                [chat],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn upgrade_keeps_every_existing_title_and_only_frees_placeholders() {
        let mut connection = resume_link_with_chats();
        apply(&mut connection).unwrap();
        assert_eq!(source(&connection, "placeholder"), "default");
        assert_eq!(source(&connection, "named"), "user");
        let titles = connection
            .prepare("SELECT title FROM chats ORDER BY ordinal")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(titles, ["Chat: Chat", "Chat: Release notes"]);
        assert!(
            connection
                .execute("UPDATE chats SET title_source='invented'", [])
                .is_err()
        );
    }

    #[test]
    fn upgrade_is_exact_idempotent_and_requires_the_resume_link_schema() {
        let mut connection = resume_link_with_chats();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentTitleSource
        );
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            VERSION
        );
        let first = fingerprint::database_fingerprint(&connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::database_fingerprint(&connection).unwrap(),
            first
        );

        let mut v20 = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut v20).unwrap();
        workspace_changes::apply(&mut v20).unwrap();
        title_provenance::apply(&mut v20).unwrap();
        assert!(apply(&mut v20).is_err());

        let mut v23 = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut v23).unwrap();
        workspace_changes::apply(&mut v23).unwrap();
        title_provenance::apply(&mut v23).unwrap();
        session_scope::apply(&mut v23).unwrap();
        notifications::apply(&mut v23).unwrap();
        config_choices::apply(&mut v23).unwrap();
        assert!(apply(&mut v23).is_err());
    }
}
