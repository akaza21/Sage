use super::*;

pub(super) const V21: &str = include_str!("../../../migrations/021_chat_viewed_marks.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentV21 => return Ok(()),
        SchemaClass::CurrentV20 => {}
        _ => return invalid("Chat viewed marks require exact schema v20"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V21)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (21,?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 21)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV21 {
        return invalid("database did not reach exact schema v21");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_leaves_existing_chats_unviewed_and_is_exact_and_idempotent() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        title_provenance::apply(&mut connection).unwrap();
        connection.execute_batch("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
            INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat: Chat',1,1);").unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV21
        );
        let viewed: Option<i64> = connection
            .query_row("SELECT viewed_turn_at FROM chats", [], |row| row.get(0))
            .unwrap();
        assert_eq!(viewed, None);
        let first = fingerprint::database_fingerprint(&connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::database_fingerprint(&connection).unwrap(),
            first
        );
        connection
            .execute("DELETE FROM schema_migrations WHERE version=21", [])
            .unwrap();
        connection.pragma_update(None, "user_version", 20).unwrap();
        assert!(apply(&mut connection).is_err());
    }
}
