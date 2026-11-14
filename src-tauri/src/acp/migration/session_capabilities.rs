use super::*;

/// The schema version that admits a fresh capability negotiation on resume.
pub(super) const VERSION: u32 = 30;
pub(super) const SQL: &str = include_str!("../../../migrations/030_session_capabilities.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentSessionCapabilities => return Ok(()),
        SchemaClass::CurrentImportedHistory => {}
        _ => return invalid("session capabilities require the exact imported history schema"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(SQL)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (?1,?2)",
        rusqlite::params![VERSION, now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", VERSION)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentSessionCapabilities {
        return invalid("database did not reach the exact session capabilities schema");
    }
    Ok(())
}
