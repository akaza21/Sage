use super::*;

pub(super) const VERSION: u32 = 34;
pub(super) const SQL: &str = include_str!("../../../migrations/034_live_session_capabilities.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentLiveSessionCapabilities => return Ok(()),
        SchemaClass::CurrentSignInEvidenceIndexes => {}
        _ => {
            return invalid(
                "live session capabilities require the exact sign-in evidence index schema",
            );
        }
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
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentLiveSessionCapabilities
    {
        return invalid("database did not reach the exact live session capabilities schema");
    }
    Ok(())
}
