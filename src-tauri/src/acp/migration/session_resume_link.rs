use super::*;

/// The schema version that adds the session resume link. The migration's
/// file name carries the same number; nothing else names it.
pub(super) const VERSION: u32 = 24;
pub(super) const SQL: &str = include_str!("../../../migrations/024_session_resume_link.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentResumeLink
        | SchemaClass::CurrentTitleSource
        | SchemaClass::CurrentSessionFactsTurnIndex => return Ok(()),
        SchemaClass::CurrentV23 => {}
        _ => return invalid("session resume links require exact schema v23"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(SQL)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (?1,?2)",
        rusqlite::params![VERSION, now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", VERSION)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentResumeLink {
        return invalid("database did not reach the exact session resume link schema");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upgraded() -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        title_provenance::apply(&mut connection).unwrap();
        session_scope::apply(&mut connection).unwrap();
        notifications::apply(&mut connection).unwrap();
        config_choices::apply(&mut connection).unwrap();
        connection
    }

    #[test]
    fn resume_link_upgrade_is_exact_idempotent_and_refuses_a_conflicting_column() {
        let mut connection = upgraded();
        apply(&mut connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentResumeLink
        );
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            VERSION
        );

        let mut conflict = upgraded();
        conflict
            .execute_batch("ALTER TABLE agent_sessions ADD COLUMN resumes_session_id TEXT;")
            .unwrap();
        assert!(apply(&mut conflict).is_err());
    }
}
