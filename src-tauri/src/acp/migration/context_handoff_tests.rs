use super::*;

#[test]
fn v20_upgrade_preserves_history_and_heads() {
    let mut connection = Connection::open_in_memory().unwrap();
    schema_upgrade::build_fresh_v18(&mut connection).unwrap();
    workspace_changes::apply(&mut connection).unwrap();
    connection.execute_batch(r#"INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','P',1,1,1);
        INSERT INTO chats(id,project_id,title,ordinal,created_at,updated_at) VALUES('chat','project','Chat',0,1,1);
        INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,state,residency,created_at,updated_at) VALUES('session','project','chat',0,'binding','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','{}','ready','cold',1,1);
        INSERT INTO chat_session_heads VALUES('project','chat','session',0,1);
        INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at,terminal_at,exit_kind) VALUES('project','chat','session',1,'exited',1,2,'parked');
        INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,created_at,updated_at,terminal_at) VALUES('turn','project','chat','session',1,0,'intent','completed','correlated',1,2,2);
        INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,created_at,updated_at,terminal_at) VALUES('item','project','chat','session','turn',1,0,'primary','text','completed','{"kind":"text","payload":{"text":"prompt"}}',43,'b2bf66ef982a206f5552f31203f5825ec8155829efc5a4d0f63ba817a6dbe9fe',1,2,2);"#).unwrap();
    chat_attention::apply(&mut connection).unwrap();
    context_handoff::apply(&mut connection).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV21
    );
    assert_eq!(
        connection
            .query_row("SELECT session_id FROM chat_session_heads", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "session"
    );
    assert_eq!(
        connection
            .query_row("SELECT payload_json FROM items", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "{\"kind\":\"text\",\"payload\":{\"text\":\"prompt\"}}"
    );
    let before = database_fingerprint(&connection).unwrap();
    context_handoff::apply(&mut connection).unwrap();
    assert_eq!(database_fingerprint(&connection).unwrap(), before);
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM acp_context_handoffs", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
}
