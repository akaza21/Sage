use super::*;

/// The schema version that indexes session facts by Turn. The migration's
/// file name carries the same number; nothing else names it.
pub(super) const VERSION: u32 = 26;
pub(super) const SQL: &str = include_str!("../../../migrations/026_session_facts_turn_index.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentSessionFactsTurnIndex
        | SchemaClass::CurrentOperationTerminalIndex
        | SchemaClass::CurrentTurnUndoIntentIndexes
        | SchemaClass::CurrentImportedHistory
        | SchemaClass::CurrentSessionCapabilities => {
            return Ok(());
        }
        SchemaClass::CurrentTitleSource => {}
        _ => return invalid("the session facts Turn index requires the exact title source schema"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(SQL)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (?1,?2)",
        rusqlite::params![VERSION, now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", VERSION)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentSessionFactsTurnIndex {
        return invalid("database did not reach the exact session facts Turn index schema");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title_source_schema() -> Connection {
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
    fn upgrade_adds_the_turn_index_exactly_once_after_title_sources() {
        let mut connection = title_source_schema();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentSessionFactsTurnIndex
        );
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            VERSION
        );
        let columns = connection
            .prepare("SELECT name FROM pragma_index_info('idx_session_facts_turn') ORDER BY seqno")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(columns, ["session_id", "turn_id", "kind", "sequence"]);
        let first = fingerprint::database_fingerprint(&connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::database_fingerprint(&connection).unwrap(),
            first
        );

        let mut v24 = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut v24).unwrap();
        workspace_changes::apply(&mut v24).unwrap();
        title_provenance::apply(&mut v24).unwrap();
        session_scope::apply(&mut v24).unwrap();
        notifications::apply(&mut v24).unwrap();
        config_choices::apply(&mut v24).unwrap();
        session_resume_link::apply(&mut v24).unwrap();
        assert!(apply(&mut v24).is_err());
    }
}
