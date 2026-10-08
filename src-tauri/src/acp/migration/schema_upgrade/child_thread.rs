use super::*;

pub(in crate::acp::migration) fn apply_through_v14(connection: &mut Connection) -> AppResult<()> {
    apply_through_v13(connection)?;
    apply_v14(connection)
}

pub(in crate::acp::migration) fn apply_through_v15(connection: &mut Connection) -> AppResult<()> {
    apply_through_v14(connection)?;
    apply_v15(connection)
}

pub(in crate::acp::migration) fn apply_v15(connection: &mut Connection) -> AppResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V15)?;
    #[cfg(test)]
    super::super::test_failpoint::hit("v14-to-v15-after-schema")?;
    transaction.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (15, ?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 15)?;
    verify_v8_foreign_keys(&transaction)?;
    transaction.commit()?;
    verify_reached(connection, SchemaClass::CurrentV15)
}

pub(in crate::acp::migration) fn build_fresh_v15(connection: &mut Connection) -> AppResult<()> {
    build_fresh_v14(connection)?;
    apply_v15(connection)
}

pub(in crate::acp::migration) fn apply_through_v16(connection: &mut Connection) -> AppResult<()> {
    apply_through_v15(connection)?;
    apply_v16(connection)
}

pub(in crate::acp::migration) fn apply_v16(connection: &mut Connection) -> AppResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V16)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (16, ?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 16)?;
    verify_v8_foreign_keys(&transaction)?;
    transaction.commit()?;
    verify_reached(connection, SchemaClass::CurrentV16)
}

pub(in crate::acp::migration) fn build_fresh_v16(connection: &mut Connection) -> AppResult<()> {
    build_fresh_v15(connection)?;
    apply_v16(connection)
}

pub(in crate::acp::migration) fn upgrade_current_through_v9(
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
        SchemaClass::CurrentV14 | SchemaClass::CurrentV15 => {}
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
            | SchemaClass::CurrentV14
            | SchemaClass::CurrentV15
    ) {
        apply_v11(&mut connection)?;
    }
    if !matches!(
        source_schema,
        SchemaClass::CurrentV11
            | SchemaClass::CurrentV12
            | SchemaClass::CurrentV13
            | SchemaClass::CurrentV14
            | SchemaClass::CurrentV15
    ) {
        apply_v12(&mut connection)?;
        apply_v13(&mut connection)?;
    }
    if !matches!(
        source_schema,
        SchemaClass::CurrentV13 | SchemaClass::CurrentV14 | SchemaClass::CurrentV15
    ) {
        apply_v14(&mut connection)?;
    }
    if source_schema != SchemaClass::CurrentV15 {
        apply_v15(&mut connection)?;
    }
    apply_v16(&mut connection)?;
    anchor.verify_path(&paths.live)?;
    Ok(opened_database(connection, anchor))
}
