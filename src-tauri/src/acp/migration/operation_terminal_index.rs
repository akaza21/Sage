use super::*;

/// The schema version that keys browser operation terminals by their start.
/// The migration's file name carries the same number; nothing else names it.
pub(super) const VERSION: u32 = 27;
pub(super) const SQL: &str =
    include_str!("../../../migrations/027_browser_operation_terminal_index.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentOperationTerminalIndex => return Ok(()),
        SchemaClass::CurrentSessionFactsTurnIndex => {}
        _ => {
            return invalid(
                "the browser operation terminal index requires the exact session facts Turn index schema",
            );
        }
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(SQL)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (?1,?2)",
        rusqlite::params![VERSION, now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", VERSION)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentOperationTerminalIndex {
        return invalid("database did not reach the exact browser operation terminal index schema");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn_index_schema() -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        title_provenance::apply(&mut connection).unwrap();
        session_scope::apply(&mut connection).unwrap();
        notifications::apply(&mut connection).unwrap();
        config_choices::apply(&mut connection).unwrap();
        session_resume_link::apply(&mut connection).unwrap();
        title_source::apply(&mut connection).unwrap();
        connection
    }

    #[test]
    fn upgrade_adds_the_terminal_index_exactly_once_after_the_turn_index() {
        let mut connection = turn_index_schema();
        assert!(apply(&mut connection).is_err());

        session_facts_turn_index::apply(&mut connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentOperationTerminalIndex
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
    }
}
