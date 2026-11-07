use super::*;

pub(super) const V21: &str = include_str!("../../../migrations/021_turn_notifications.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentV21 => return Ok(()),
        SchemaClass::CurrentV20 => {}
        _ => return invalid("turn notifications require exact schema v20"),
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
    fn notification_upgrade_is_exact_idempotent_and_refuses_a_conflicting_table() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        title_provenance::apply(&mut connection).unwrap();
        apply(&mut connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV21
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
        conflict
            .execute_batch("CREATE TABLE turn_notification_receipts(unrelated TEXT);")
            .unwrap();
        assert!(apply(&mut conflict).is_err());
        assert_eq!(
            conflict
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            20
        );
    }
}
