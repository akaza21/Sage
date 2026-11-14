use super::*;
use sage_db::error::SqliteResultExt;

pub(super) const VERSION: u32 = 28;
pub(super) const SQL: &str = include_str!("../../../migrations/028_turn_undo_intent_indexes.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentTurnUndoIntentIndexes | SchemaClass::CurrentImportedHistory => {
            return Ok(());
        }
        SchemaClass::CurrentOperationTerminalIndex => {}
        _ => {
            return invalid(
                "undo intent indexes require the exact browser operation terminal index schema",
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
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentTurnUndoIntentIndexes {
        return invalid("database did not reach the exact undo intent index schema");
    }
    Ok(())
}
