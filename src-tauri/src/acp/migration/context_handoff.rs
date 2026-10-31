use super::*;

pub(super) const V21: &str = include_str!("../../../migrations/021_context_handoff.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentV21 => return Ok(()),
        SchemaClass::CurrentV20 => {}
        _ => return invalid("context handoff requires exact schema v20"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V21)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES(21,?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 21)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV21 {
        return invalid("database did not reach exact schema v21");
    }
    Ok(())
}
