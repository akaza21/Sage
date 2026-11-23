use super::*;

pub(super) const VERSION: u32 = 31;
pub(super) const SQL: &str = include_str!("../../../migrations/031_acp_child_thread_states.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentChildThreadStates
        | SchemaClass::CurrentPrivateProjects
        | SchemaClass::CurrentSignInEvidenceIndexes
        | SchemaClass::CurrentAgentFileWrites
        | SchemaClass::CurrentCatalogCapabilities => {
            return Ok(());
        }
        SchemaClass::CurrentSessionCapabilities => {}
        _ => return invalid("child thread states require the exact session capabilities schema"),
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
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentChildThreadStates {
        return invalid("database did not reach the exact child thread states schema");
    }
    Ok(())
}
