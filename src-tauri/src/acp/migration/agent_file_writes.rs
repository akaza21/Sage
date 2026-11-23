use super::*;

pub(super) const VERSION: u32 = 34;
pub(super) const SQL: &str = include_str!("../../../migrations/034_acp_agent_file_writes.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentAgentFileWrites | SchemaClass::CurrentCatalogCapabilities => {
            return Ok(());
        }
        SchemaClass::CurrentSignInEvidenceIndexes => {}
        _ => return invalid("agent file writes require the exact sign-in evidence index schema"),
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
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentAgentFileWrites {
        return invalid("database did not reach the exact agent file writes schema");
    }
    Ok(())
}
