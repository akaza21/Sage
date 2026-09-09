use super::*;

pub(super) fn apply_through_v9(connection: &mut Connection) -> AppResult<()> {
    let versions = connection
        .prepare("SELECT version FROM schema_migrations ORDER BY version LIMIT 10")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if versions.is_empty()
        || versions.len() > 8
        || versions
            .iter()
            .enumerate()
            .any(|(index, version)| *version != index as i64 + 1)
    {
        return invalid("replacement source has unsupported migration history");
    }
    let last = versions.last().copied().unwrap_or(0);
    if last < 2 {
        connection.execute_batch(V2)?;
    }
    if last < 3 {
        connection.execute_batch(V3)?;
    }
    if last < 4 {
        project_migration::migrate(connection)?;
    }
    if last < 5 {
        apply_v5(connection)?;
    }
    if last < 6 {
        apply_v6_and_v7_atomically(connection)?;
        apply_v8(connection)?;
        apply_v9(connection)?;
        return apply_v10(connection);
    }
    if last < 7 {
        apply_v7(connection)?;
        apply_v8(connection)?;
        apply_v9(connection)?;
        return apply_v10(connection);
    }
    if last < 8 {
        apply_v8(connection)?;
    }
    apply_v9(connection)?;
    apply_v10(connection)
}

pub(super) fn apply_v5(connection: &mut Connection) -> AppResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    legacy::convert(&transaction, now_millis())?;
    let foreign_key_error = transaction
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    if foreign_key_error {
        return invalid("ACP migration failed its foreign-key check");
    }
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV5 {
        return invalid("database did not reach exact schema v5");
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn apply_v6(connection: &mut Connection) -> AppResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    apply_v6_steps(&transaction)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV6 {
        return invalid("database did not reach exact schema v6");
    }
    Ok(())
}

pub(super) fn apply_v7(connection: &mut Connection) -> AppResult<()> {
    with_foreign_keys_disabled(connection, |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        apply_v7_steps(&transaction)?;
        verify_v7_foreign_keys(&transaction)?;
        transaction.commit()?;
        Ok(())
    })?;
    verify_reached(connection, SchemaClass::CurrentV7)
}

pub(super) fn apply_v6_and_v7_atomically(connection: &mut Connection) -> AppResult<()> {
    with_foreign_keys_disabled(connection, |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        apply_v6_steps(&transaction)?;
        #[cfg(test)]
        super::test_failpoint::hit("v5-to-v7-after-v6")?;
        apply_v7_steps(&transaction)?;
        verify_v7_foreign_keys(&transaction)?;
        transaction.commit()?;
        Ok(())
    })?;
    verify_reached(connection, SchemaClass::CurrentV7)
}

pub(super) fn apply_v8(connection: &mut Connection) -> AppResult<()> {
    with_foreign_keys_disabled(connection, |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        apply_v8_steps(&transaction)?;
        verify_v8_foreign_keys(&transaction)?;
        transaction.commit()?;
        Ok(())
    })?;
    verify_reached(connection, SchemaClass::CurrentV8)
}

pub(super) fn apply_v9(connection: &mut Connection) -> AppResult<()> {
    with_foreign_keys_disabled(connection, |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(V9)?;
        #[cfg(test)]
        super::test_failpoint::hit("v8-to-v9-after-schema")?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (9, ?1)",
            [now_millis()],
        )?;
        transaction.pragma_update(None, "user_version", 9)?;
        verify_v8_foreign_keys(&transaction)?;
        transaction.commit()?;
        Ok(())
    })?;
    verify_reached(connection, SchemaClass::CurrentV9)
}

pub(super) fn apply_v10(connection: &mut Connection) -> AppResult<()> {
    with_foreign_keys_disabled(connection, |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(V10)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (10, ?1)",
            [now_millis()],
        )?;
        transaction.pragma_update(None, "user_version", 10)?;
        verify_v8_foreign_keys(&transaction)?;
        transaction.commit()?;
        Ok(())
    })?;
    verify_current(connection)
}

fn with_foreign_keys_disabled(
    connection: &mut Connection,
    migration: impl FnOnce(&mut Connection) -> AppResult<()>,
) -> AppResult<()> {
    let foreign_keys: bool =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    if foreign_keys {
        connection.pragma_update(None, "foreign_keys", "OFF")?;
    }
    let result = migration(connection);
    if foreign_keys {
        connection.pragma_update(None, "foreign_keys", "ON")?;
    }
    result
}

fn apply_v6_steps(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    transaction.execute_batch(V6)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (6, ?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 6)?;
    Ok(())
}

fn apply_v7_steps(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    transaction.execute_batch(V7)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (7, ?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 7)?;
    Ok(())
}

fn apply_v8_steps(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    transaction.execute_batch(V8)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (8, ?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 8)?;
    Ok(())
}

fn verify_reached(connection: &Connection, class: SchemaClass) -> AppResult<()> {
    if fingerprint::classify_connection(connection)? != class {
        return invalid("database did not reach the expected schema version");
    }
    Ok(())
}

fn verify_v8_foreign_keys(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    let foreign_key_error = transaction
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    if foreign_key_error {
        return invalid("ACP permission-decision migration failed its foreign-key check");
    }
    Ok(())
}

fn verify_v7_foreign_keys(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    let foreign_key_error = transaction
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    if foreign_key_error {
        return invalid("ACP activation-abort migration failed its foreign-key check");
    }
    Ok(())
}

pub(super) fn build_fresh_v9(connection: &mut Connection) -> AppResult<()> {
    build_fresh_v10(connection)
}

pub(super) fn build_fresh_v10(connection: &mut Connection) -> AppResult<()> {
    connection.execute_batch(INITIAL)?;
    connection.execute_batch(V2)?;
    connection.execute_batch(V3)?;
    project_migration::migrate(connection)?;
    apply_v5(connection)?;
    apply_v6_and_v7_atomically(connection)?;
    apply_v8(connection)?;
    apply_v9(connection)?;
    apply_v10(connection)
}
