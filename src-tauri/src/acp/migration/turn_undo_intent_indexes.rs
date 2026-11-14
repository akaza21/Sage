use super::*;

pub(super) const VERSION: u32 = 28;
pub(super) const SQL: &str = include_str!("../../../migrations/028_turn_undo_intent_indexes.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentTurnUndoIntentIndexes
        | SchemaClass::CurrentImportedHistory
        | SchemaClass::CurrentSessionCapabilities => {
            return Ok(());
        }
        SchemaClass::CurrentOperationTerminalIndex => {}
        _ => {
            return invalid(
                "undo intent indexes require the exact browser operation terminal index schema",
            );
        }
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(SQL)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (?1,?2)",
        rusqlite::params![VERSION, now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", VERSION)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentTurnUndoIntentIndexes {
        return invalid("database did not reach the exact undo intent index schema");
    }
    Ok(())
}
