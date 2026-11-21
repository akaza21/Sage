use super::*;

pub(super) const VERSION: u32 = 32;
pub(super) const SQL: &str = include_str!("../../../migrations/032_private_chat_projects.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentPrivateProjects | SchemaClass::CurrentSignInEvidenceIndexes => {
            return Ok(());
        }
        SchemaClass::CurrentChildThreadStates => {}
        _ => return invalid("private projects require the exact child thread states schema"),
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
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentPrivateProjects {
        return invalid("database did not reach the exact private projects schema");
    }
    Ok(())
}
