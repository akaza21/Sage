use super::*;

pub(super) const VERSION: u32 = 35;
pub(super) const SQL: &str = include_str!("../../../migrations/035_catalog_capabilities.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentCatalogCapabilities => return Ok(()),
        SchemaClass::CurrentAgentFileWrites => {}
        _ => return invalid("catalog capabilities require the exact agent file writes schema"),
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
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentCatalogCapabilities {
        return invalid("database did not reach the exact catalog capabilities schema");
    }
    Ok(())
}
