use super::*;

/// The schema version that adds imported session histories. The migration's
/// file name carries the same number; nothing else names it.
pub(super) const VERSION: u32 = 25;
pub(super) const SQL: &str = include_str!("../../../migrations/025_imported_session_history.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentImportedHistory => return Ok(()),
        SchemaClass::CurrentResumeLink => {}
        _ => return invalid("imported session histories require the exact resume link schema"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(SQL)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (?1,?2)",
        rusqlite::params![VERSION, now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", VERSION)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentImportedHistory {
        return invalid("database did not reach the exact imported history schema");
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
        session_resume_link::apply(&mut connection).unwrap();
        connection
    }

    #[test]
    fn imported_history_upgrade_is_exact_idempotent_and_refuses_a_conflicting_table() {
        let mut connection = upgraded();
        apply(&mut connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentImportedHistory
        );
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            VERSION
        );

        let mut conflict = upgraded();
        conflict
            .execute_batch("CREATE TABLE acp_imported_histories(session_id TEXT);")
            .unwrap();
        assert!(apply(&mut conflict).is_err());
    }
}
