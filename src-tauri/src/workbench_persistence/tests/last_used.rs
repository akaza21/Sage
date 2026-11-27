use super::*;

const CAPABILITIES: &str = r#"{"schemaVersion":1,"protocolVersion":"acp-1","newSession":true,"resumeSession":false,"loadSession":false,"closeSession":true,"prompt":true,"cancel":true,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":1,"configCount":0}"#;

/// A closed Codex session in the project's chat, which leaves the open chat
/// itself without a session, as a new chat is.
fn database_after_a_codex_chat() -> Database {
    let (database, project_id, chat_id) = database_with_project();
    let connection = database.connection();
    connection
        .execute(
            "INSERT INTO agent_sessions(
               id,project_id,chat_id,ordinal,binding_id,binding_digest,model_id,
               capabilities_json,state,residency,legacy,revision,projection_revision,
               created_at,updated_at,terminal_at
             ) VALUES ('ended',?1,?2,1,'codex-app-server',?3,'codex-model',
                       ?4,'closed','cold',0,0,0,2,2,2)",
            params![project_id, chat_id, "a".repeat(64), CAPABILITIES],
        )
        .unwrap();
    for ledger in [
        "INSERT INTO acp_session_usage(
           project_id,chat_id,session_id,turn_count,item_count,item_bytes,
           fact_count,fact_bytes,payload_bytes,revision
         ) VALUES (?1,?2,'ended',0,0,0,0,0,0,0)",
        "UPDATE acp_project_usage SET session_count=session_count+1
         WHERE project_id=?1 AND ?2 IS NOT NULL",
        "UPDATE acp_store_usage SET session_count=session_count+1
         WHERE singleton=1 AND ?1 IS NOT NULL AND ?2 IS NOT NULL",
    ] {
        connection
            .execute(ledger, params![project_id, chat_id])
            .unwrap();
    }
    drop(connection);
    database
}

#[test]
fn a_new_chat_starts_on_the_harness_and_model_used_last_while_it_is_still_checked() {
    let database = database_after_a_codex_chat();
    // Discovery has not settled, so Codex is still being checked.
    let snapshot = database
        .bootstrap_snapshot_with_catalog(&HarnessCatalog::default())
        .unwrap();
    assert_eq!(snapshot.session_id, None);
    assert_eq!(snapshot.selected_harness_id.as_deref(), Some("codex"));
    assert_eq!(snapshot.selected_model_id.as_deref(), Some("codex-model"));
}

#[test]
fn a_harness_used_last_that_discovery_did_not_find_is_not_chosen() {
    let database = database_after_a_codex_chat();
    let snapshot = database
        .bootstrap_snapshot_with_catalog(&HarnessCatalog::default().settled_for_test())
        .unwrap();
    assert_ne!(snapshot.selected_harness_id.as_deref(), Some("codex"));
    assert_eq!(snapshot.selected_model_id, None);
}
