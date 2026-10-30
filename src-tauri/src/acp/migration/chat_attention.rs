use super::*;

pub(super) const V20: &str = include_str!("../../../migrations/020_chat_attention.sql");

pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    if fingerprint::classify_connection(connection)? == SchemaClass::CurrentV20 {
        return Ok(());
    }
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV19 {
        return invalid("chat attention requires exact schema v19");
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(V20)?;
    let mut after = String::new();
    loop {
        let rows = transaction
            .prepare("SELECT project_id,id FROM chats WHERE id>?1 ORDER BY id LIMIT 64")?
            .query_map([&after], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            break;
        }
        for (project, chat) in &rows {
            crate::chat_attention::store::refresh(&transaction, project, chat)
                .map_err(|_| AppError::InvalidRequest("attention backfill failed".into()))?;
            transaction.execute(
                "INSERT INTO chat_attention(chat_id) VALUES(?1) ON CONFLICT DO NOTHING",
                [chat],
            )?;
        }
        after = rows.last().expect("nonempty batch").1.clone();
    }
    transaction.execute("UPDATE chat_attention SET activity_position=CASE WHEN activity_position>0 THEN 1 ELSE 0 END,
        terminal_position=CASE WHEN terminal_position IS NOT NULL THEN 1 END,revision=CASE WHEN revision>0 THEN 1 ELSE 0 END", [])?;
    transaction.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES(20,?1)",
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
pub(crate) fn assert_v19_upgrade() {
    let mut connection = Connection::open_in_memory().unwrap();
    schema_upgrade::build_fresh_v18(&mut connection).unwrap();
    workspace_changes::apply(&mut connection).unwrap();
    connection.execute_batch("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
      INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat',1,1);
      INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,state,residency,created_at,updated_at) VALUES('session','project','chat',0,'binding','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','{}','ready','hot',1,1);
      INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project','chat','session',1,'running','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',1);
      INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,created_at,updated_at,terminal_at) VALUES('turn','project','chat','session',1,0,'intent','completed','correlated',1,2,2);").unwrap();
    let before = fingerprint::database_fingerprint(&connection).unwrap();
    {
        let transaction = connection.transaction().unwrap();
        transaction.execute_batch(V20).unwrap();
    }
    assert_eq!(
        fingerprint::database_fingerprint(&connection).unwrap(),
        before
    );
    apply(&mut connection).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV20
    );
    let summary = crate::chat_attention::store::get(&connection, "project", &["chat".into()])
        .unwrap()
        .remove(0);
    assert_eq!(summary.activity_position.get(), 1);
    assert_eq!(summary.terminal_position.unwrap().get(), 1);
    assert!(summary.read_position.is_none());
    apply(&mut connection).unwrap();
    connection.pragma_update(None, "user_version", 19).unwrap();
    assert!(apply(&mut connection).is_err());
}
