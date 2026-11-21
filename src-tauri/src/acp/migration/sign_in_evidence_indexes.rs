use super::*;

pub(super) const VERSION: u32 = 33;
pub(super) const SQL: &str = include_str!("../../../migrations/033_sign_in_evidence_indexes.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentSignInEvidenceIndexes => return Ok(()),
        SchemaClass::CurrentPrivateProjects => {}
        _ => return invalid("sign-in evidence indexes require the exact private projects schema"),
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
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentSignInEvidenceIndexes {
        return invalid("database did not reach the exact sign-in evidence index schema");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrades_private_projects_and_reopens_without_changing_history() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        title_provenance::apply(&mut connection).unwrap();
        session_scope::apply(&mut connection).unwrap();
        notifications::apply(&mut connection).unwrap();
        config_choices::apply(&mut connection).unwrap();
        session_resume_link::apply(&mut connection).unwrap();
        title_source::apply(&mut connection).unwrap();
        session_facts_turn_index::apply(&mut connection).unwrap();
        operation_terminal_index::apply(&mut connection).unwrap();
        turn_undo_intent_indexes::apply(&mut connection).unwrap();
        imported_history::apply(&mut connection).unwrap();
        session_capabilities::apply(&mut connection).unwrap();
        child_thread_states::apply(&mut connection).unwrap();
        private_projects::apply(&mut connection).unwrap();
        apply(&mut connection).unwrap();
        let before = database_fingerprint(&connection).unwrap();
        post_v18::apply(&mut connection).unwrap();
        let after = database_fingerprint(&connection).unwrap();
        assert_eq!(before, after);
        assert_eq!(after.schema, SchemaClass::CurrentSignInEvidenceIndexes);
    }
}
