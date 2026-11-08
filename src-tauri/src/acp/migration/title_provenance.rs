use super::*;

pub(super) const V20: &str = include_str!("../../../migrations/020_chat_title_provenance.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentV20
        | SchemaClass::CurrentV21
        | SchemaClass::CurrentV22
        | SchemaClass::CurrentV23 => {
            return Ok(());
        }
        SchemaClass::CurrentV19 => {}
        _ => return invalid("Chat title provenance requires exact schema v19"),
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V20)?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES (20,?1)",
        [now_millis()],
    )?;
    transaction.pragma_update(None, "user_version", 20)?;
    transaction.commit()?;
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV20 {
        return invalid("database did not reach exact schema v20");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_preserves_unknown_title_intent_and_is_exact_and_idempotent() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        workspace_changes::apply(&mut connection).unwrap();
        connection.execute_batch("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
            INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat: Chat',1,1);").unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV20
        );
        let row: (String, bool) = connection
            .query_row("SELECT title,title_is_default FROM chats", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(row, ("Chat: Chat".into(), false));
        let first = fingerprint::database_fingerprint(&connection).unwrap();
        apply(&mut connection).unwrap();
        assert_eq!(
            fingerprint::database_fingerprint(&connection).unwrap(),
            first
        );
        connection
            .execute("DELETE FROM schema_migrations WHERE version=20", [])
            .unwrap();
        connection.pragma_update(None, "user_version", 19).unwrap();
        assert!(apply(&mut connection).is_err());
    }
}
