use super::*;

pub(super) const V20: &str = include_str!("../../../migrations/020_context_handoff.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentV20 => return Ok(()),
        SchemaClass::CurrentV19 => {}
        _ => return invalid("context handoff requires exact schema v19"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V20)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES(20,?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 20)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV20 {
        return invalid("database did not reach exact schema v20");
    }
    Ok(())
}
