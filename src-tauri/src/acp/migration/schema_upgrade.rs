use super::*;

pub(super) fn upgrade_current_through_v9(
    paths: &MigrationPaths,
    source_schema: SchemaClass,
) -> AppResult<OpenedDatabase> {
    let verified_backup = ensure(paths)?;
    let anchor = platform::anchor_existing_sqlite(&paths.live, true)?;
    if probe::classify_live(paths)? != source_schema {
        return invalid("current SQLite path changed before its anchored upgrade");
    }
    let mut connection = Connection::open(&paths.live)?;
    if database_fingerprint(&connection)? != verified_backup.source.database {
        return invalid("current source changed after original-schema backup publication");
    }
    match source_schema {
        SchemaClass::CurrentV5 => {
            apply_v6_and_v7_atomically(&mut connection)?;
            apply_v8(&mut connection)?;
            apply_v9(&mut connection)?;
            apply_v10(&mut connection)?;
        }
        SchemaClass::CurrentV6 => {
            apply_v7(&mut connection)?;
            apply_v8(&mut connection)?;
            apply_v9(&mut connection)?;
            apply_v10(&mut connection)?;
        }
        SchemaClass::CurrentV7 => {
            apply_v8(&mut connection)?;
            apply_v9(&mut connection)?;
            apply_v10(&mut connection)?;
        }
        SchemaClass::CurrentV8 => {
            apply_v9(&mut connection)?;
            apply_v10(&mut connection)?;
        }
        SchemaClass::CurrentV9 => apply_v10(&mut connection)?,
        SchemaClass::CurrentV12 => apply_v13(&mut connection)?,
        SchemaClass::CurrentV13 => apply_v14(&mut connection)?,
        SchemaClass::CurrentV11 => {
            apply_v12(&mut connection)?;
            apply_v13(&mut connection)?;
        }
        SchemaClass::CurrentV10 | SchemaClass::CurrentV10LegacySessionEvents => {
            apply_v11(&mut connection)?
        }
        _ => return invalid("in-place upgrade source is not admitted"),
    }
    if !matches!(
        source_schema,
        SchemaClass::CurrentV10
            | SchemaClass::CurrentV10LegacySessionEvents
            | SchemaClass::CurrentV11
            | SchemaClass::CurrentV12
            | SchemaClass::CurrentV13
    ) {
        apply_v11(&mut connection)?;
    }
    if !matches!(
        source_schema,
        SchemaClass::CurrentV11 | SchemaClass::CurrentV12 | SchemaClass::CurrentV13
    ) {
        apply_v12(&mut connection)?;
        apply_v13(&mut connection)?;
    }
    if source_schema != SchemaClass::CurrentV13 {
        apply_v14(&mut connection)?;
    }
    anchor.verify_path(&paths.live)?;
    Ok(opened_database(connection, anchor))
}

pub(super) fn apply_through_v11(connection: &mut Connection) -> AppResult<()> {
    let versions = connection
        .prepare("SELECT version FROM schema_migrations ORDER BY version LIMIT 11")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if versions.is_empty()
        || versions.len() > 11
        || versions
            .iter()
            .enumerate()
            .any(|(index, version)| *version != index as i64 + 1)
    {
        return invalid("replacement source has unsupported migration history");
    }
    let last = versions.last().copied().unwrap_or(0);
    if last == 11 {
        return Ok(());
    }
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
        apply_v10(connection)?;
        return apply_v11(connection);
    }
    if last < 7 {
        apply_v7(connection)?;
        apply_v8(connection)?;
        apply_v9(connection)?;
        apply_v10(connection)?;
        return apply_v11(connection);
    }
    if last < 8 {
        apply_v8(connection)?;
    }
    if last < 9 {
        apply_v9(connection)?;
    }
    if last < 10 {
        apply_v10(connection)?;
    }
    apply_v11(connection)
}

pub(super) fn apply_through_v12(connection: &mut Connection) -> AppResult<()> {
    apply_through_v11(connection)?;
    apply_v12(connection)
}

pub(super) fn apply_through_v13(connection: &mut Connection) -> AppResult<()> {
    apply_through_v12(connection)?;
    apply_v13(connection)
}

pub(super) fn apply_through_v14(connection: &mut Connection) -> AppResult<()> {
    apply_through_v13(connection)?;
    apply_v14(connection)
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
    verify_reached(connection, SchemaClass::CurrentV10)
}

pub(super) fn apply_v11(connection: &mut Connection) -> AppResult<()> {
    with_foreign_keys_disabled(connection, |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(V11)?;
        tool_result_fold::apply(&transaction)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (11, ?1)",
            [now_millis()],
        )?;
        transaction.pragma_update(None, "user_version", 11)?;
        verify_v8_foreign_keys(&transaction)?;
        transaction.commit()?;
        Ok(())
    })?;
    verify_reached(connection, SchemaClass::CurrentV11)
}

pub(super) fn apply_v12(connection: &mut Connection) -> AppResult<()> {
    with_foreign_keys_disabled(connection, |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(V12)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (12, ?1)",
            [now_millis()],
        )?;
        transaction.pragma_update(None, "user_version", 12)?;
        verify_v8_foreign_keys(&transaction)?;
        transaction.commit()?;
        Ok(())
    })?;
    verify_reached(connection, SchemaClass::CurrentV12)
}

pub(super) fn apply_v13(connection: &mut Connection) -> AppResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V13)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (13, ?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 13)?;
    verify_v8_foreign_keys(&transaction)?;
    transaction.commit()?;
    verify_reached(connection, SchemaClass::CurrentV13)
}

pub(super) fn apply_v14(connection: &mut Connection) -> AppResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V14)?;
    #[cfg(test)]
    super::test_failpoint::hit("v13-to-v14-after-schema")?;
    transaction.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (14, ?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 14)?;
    verify_v8_foreign_keys(&transaction)?;
    transaction.commit()?;
    verify_reached(connection, SchemaClass::CurrentV14)
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

pub(super) fn build_fresh_v11(connection: &mut Connection) -> AppResult<()> {
    build_fresh_v10(connection)?;
    apply_v11(connection)
}

pub(super) fn build_fresh_v12(connection: &mut Connection) -> AppResult<()> {
    build_fresh_v11(connection)?;
    apply_v12(connection)
}

pub(super) fn build_fresh_v13(connection: &mut Connection) -> AppResult<()> {
    build_fresh_v12(connection)?;
    apply_v13(connection)
}

pub(super) fn build_fresh_v14(connection: &mut Connection) -> AppResult<()> {
    build_fresh_v13(connection)?;
    apply_v14(connection)
}

#[cfg(test)]
pub(super) fn build_fresh_v9(connection: &mut Connection) -> AppResult<()> {
    connection.execute_batch(INITIAL)?;
    connection.execute_batch(V2)?;
    connection.execute_batch(V3)?;
    project_migration::migrate(connection)?;
    apply_v5(connection)?;
    apply_v6_and_v7_atomically(connection)?;
    apply_v8(connection)?;
    apply_v9(connection)
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
