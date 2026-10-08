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
