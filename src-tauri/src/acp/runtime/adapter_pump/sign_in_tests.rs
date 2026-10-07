use super::*;

use crate::{
    acp::{
        runtime::{JournalHandle, ProjectionMailbox, SqliteAdapterBatchJournal},
        store::{DatabaseAcpStore, load_chat_page},
    },
    persistence::Database,
};

#[test]
fn a_sign_in_refusal_fails_the_durable_turn_through_the_migrated_store() {
    runtime().block_on(async {
        let database = migrated_database();
        let intent = session_intent().bind_turn(ActiveTurnIntent {
            turn_id: turn("one"),
            external_turn_id: Some(ExternalId::new("turn", "external-turn").unwrap()),
            client_message_id: message(),
        });
        let shared = Shared(Arc::new(Mutex::new(FakeState {
            ingest_error: Some(AdapterRuntimeError::SignInRequired),
            refusal_detail: RefusalDetail::new(
                Some("authentication required"),
                Some("Sign in with your Claude account"),
            ),
            ..FakeState::default()
        })));
        let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let task = runner.spawn();
        let mut pump = AdapterPump::new(
            FakeAdapter {
                shared: shared.clone(),
                next_request: 0,
            },
            SqliteAdapterBatchJournal::new(handle.clone(), intent.clone()),
            Mapper,
            Arc::new(ProjectionMailbox::default()),
            intent,
        )
        .unwrap();

        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 9).await,
            Err(AdapterPumpError::Refused(NonfatalRefusal::SignIn))
        ));
        handle.close_when_drained();
        task.await.unwrap();

        let connection = database.connection();
        let stored: (String, Option<i64>) = connection
            .query_row(
                "SELECT state,terminal_at FROM turns WHERE id='turn-one'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, ("failed".to_owned(), Some(9)));
        let page = load_chat_page(
            &connection,
            SageProjectId::parse("project-one").unwrap(),
            SageChatId::parse("chat-one").unwrap(),
            None,
        )
        .unwrap();
        let projected = page
            .turns
            .as_slice()
            .iter()
            .map(|value| value.state)
            .collect::<Vec<_>>();
        assert_eq!(projected, [TurnState::Failed]);
        assert_eq!(
            page.turns.as_slice()[0].terminal_reason,
            Some(crate::acp::projection::TurnTerminalReasonProjection {
                kind: crate::acp::lifecycle::TurnTerminalReason::AuthRequired,
                message: Some("authentication required".to_owned()),
                remediation: Some("Sign in with your Claude account".to_owned()),
            })
        );
    });
}

fn migrated_database() -> Database {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let connection = database.connection();
    connection.execute_batch(&format!(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Sign in',1,1,1);
         INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-one','project-one','C:/fixture','c:/fixture',0);
         INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Sign in',0,1,1);
         INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-one','project-one','chat-one',0,'binding','{digest}','external-session','{capabilities}','active','hot',0,0,0,1,1);
         INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1);
         INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-one','chat-one','session-one',1,'running','{artifact}',1);
         INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-one','project-one','chat-one','session-one',1,0,'intent-one','running','correlated',0,1,1);
         INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,activity_id,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at) VALUES('item-one','project-one','chat-one','session-one','turn-one',1,0,'primary',NULL,'text','completed',NULL,'item-one','{{\"kind\":\"text\",\"payload\":{{\"text\":\"prompt\"}}}}',43,'b2bf66ef982a206f5552f31203f5825ec8155829efc5a4d0f63ba817a6dbe9fe',0,1,1,1);
         INSERT INTO acp_project_usage VALUES('project-one',1,1,1,0,0,0,0);
         INSERT INTO acp_session_usage VALUES('project-one','chat-one','session-one',1,0,0,0,0,0,0);
         UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=1 WHERE singleton=1;",
        digest = "07".repeat(32),
        artifact = "d".repeat(64),
        capabilities = serde_json::to_string(&pending_capabilities()).unwrap(),
    ))
    .unwrap();
    drop(connection);
    database
}

fn pending_capabilities() -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocol", "1").unwrap(),
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        config_count: 0,
    }
}
