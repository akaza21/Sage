use super::*;

pub(super) const VERSION: u32 = 34;
pub(super) const SQL: &str = include_str!("../../../migrations/034_chat_retention.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    match fingerprint::classify_connection(connection)? {
        SchemaClass::CurrentChatRetention => return Ok(()),
        SchemaClass::CurrentSignInEvidenceIndexes => {}
        _ => return invalid("chat retention requires the exact sign-in evidence index schema"),
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
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentChatRetention {
        return invalid("database did not reach the exact chat retention schema");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_retains_existing_archives_without_retaining_hidden_attachments() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema_upgrade::build_fresh_v18(&mut connection).unwrap();
        post_v18::apply(&mut connection).unwrap();
        test_schema::rollback_to_version(&connection, 33).unwrap();
        connection.execute_batch(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
               VALUES('project','Project',1,1,1);
             INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at,title_source)
               VALUES('archived','project',0,'Chat: Chat',1,2,'default'),
                     ('hidden','project',1,'Chat: Chat',1,3,'default');
             INSERT INTO workbench_tabs(id,project_id,kind,title,position,pinned,dirty,status,metadata_json,created_at,updated_at)
               VALUES('hidden','project','chat','Chat: Chat',0,0,0,'idle','{}',1,3);
             INSERT INTO chat_tabs(project_id,chat_id,tab_id,attached_at)
               VALUES('project','hidden','hidden',1);"
        ).unwrap();
        apply(&mut connection).unwrap();
        apply(&mut connection).unwrap();
        let rows = connection
            .prepare("SELECT id,retained_at FROM chats ORDER BY ordinal")
            .unwrap()
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![("archived".into(), Some(2)), ("hidden".into(), None)]
        );
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentChatRetention
        );
    }
}
