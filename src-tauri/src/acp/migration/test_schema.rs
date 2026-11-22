//! Roll current, populated fixtures back with their exact historical schema.
use super::*;

pub(crate) fn rollback_to_version(connection: &Connection, version: u32) -> AppResult<()> {
    if !(26..CURRENT_SCHEMA_VERSION).contains(&version) {
        return invalid("fixture rollback supports schemas 26 through the previous version");
    }
    if fingerprint::classify_connection(connection)? != fingerprint::CURRENT_SCHEMA {
        return invalid("fixture rollback requires the exact current schema");
    }
    let transaction = connection.unchecked_transaction().db()?;
    for (introduced, sql) in [
        (
            34,
            "DROP TRIGGER delete_agent_file_writes;
              DROP TABLE acp_agent_file_writes;",
        ),
        (
            33,
            "DROP INDEX idx_session_facts_sign_in_refusal;
              DROP INDEX idx_session_facts_session_intent;",
        ),
        (
            32,
            "DROP INDEX idx_projects_kind_created;
              ALTER TABLE projects DROP COLUMN kind;",
        ),
        (31, "DROP TABLE acp_child_thread_states;"),
        (
            29,
            "DROP TABLE acp_imported_histories;
              DROP INDEX idx_agent_sessions_external_session;",
        ),
        (
            28,
            "DROP INDEX idx_session_facts_undo_start;
              DROP INDEX idx_session_facts_undo_recovery;
              DROP INDEX idx_session_facts_undo_settled;",
        ),
        (
            27,
            "DROP INDEX idx_session_facts_operation_start;
              DROP INDEX idx_session_facts_operation_terminal;",
        ),
    ] {
        if version < introduced {
            transaction.execute_batch(sql).db()?;
        }
    }
    if version < 30 {
        let trigger = historical_trigger_sql(version, "deny_agent_session_identity_mutation")?;
        transaction
            .execute_batch(&format!(
                "DROP TRIGGER deny_agent_session_identity_mutation; {trigger};"
            ))
            .db()?;
    }
    transaction
        .execute(
            "DELETE FROM schema_migrations WHERE version > ?1",
            [version],
        )
        .db()?;
    transaction
        .pragma_update(None, "user_version", version)
        .db()?;
    // Validate before committing so omitted rollback steps fail at the fixture.
    fingerprint::classify_connection(&transaction)?;
    transaction.commit().db()
}
