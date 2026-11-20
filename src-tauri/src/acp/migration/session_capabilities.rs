use super::*;

pub(super) const VERSION: u32 = 30;
pub(super) const SQL: &str = include_str!("../../../migrations/030_session_capabilities.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentSessionCapabilities
        | SchemaClass::CurrentChildThreadStates
        | SchemaClass::CurrentPrivateProjects => {
            return Ok(());
        }
        SchemaClass::CurrentImportedHistory => {}
        _ => return invalid("session capabilities require the exact imported history schema"),
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .db()?;
    transaction.execute_batch(SQL).db()?;
    transaction
        .execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES (?1,?2)",
            rusqlite::params![VERSION, now_millis()],
        )
        .db()?;
    transaction
        .pragma_update(None, "user_version", VERSION)
        .db()?;
    transaction.commit().db()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentSessionCapabilities {
        return invalid("database did not reach the exact session capabilities schema");
    }
    Ok(())
}
