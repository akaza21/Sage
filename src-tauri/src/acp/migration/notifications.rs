use super::*;

pub(super) const V22: &str = include_str!("../../../migrations/022_turn_notifications.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentV22
        | SchemaClass::CurrentV23
        | SchemaClass::CurrentResumeLink
        | SchemaClass::CurrentTitleSource
        | SchemaClass::CurrentSessionFactsTurnIndex
        | SchemaClass::CurrentOperationTerminalIndex
        | SchemaClass::CurrentTurnUndoIntentIndexes
        | SchemaClass::CurrentImportedHistory
        | SchemaClass::CurrentSessionCapabilities => {
            return Ok(());
        }
        SchemaClass::CurrentV21 => {}
        _ => return invalid("turn notifications require exact schema v22"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V22)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (22,?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 22)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV22 {
        return invalid("database did not reach exact schema v22");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_upgrade_is_exact_idempotent_and_refuses_a_conflicting_table() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        title_provenance::apply(&mut connection).unwrap();
        session_scope::apply(&mut connection).unwrap();
        apply(&mut connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV22
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM turn_notification_receipts",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        let mut conflict = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut conflict).unwrap();
        workspace_changes::apply(&mut conflict).unwrap();
        title_provenance::apply(&mut conflict).unwrap();
        session_scope::apply(&mut conflict).unwrap();
        conflict
            .execute_batch("CREATE TABLE turn_notification_receipts(unrelated TEXT);")
            .unwrap();
        assert!(apply(&mut conflict).is_err());
        assert_eq!(
            conflict
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            21
        );
    }
}
