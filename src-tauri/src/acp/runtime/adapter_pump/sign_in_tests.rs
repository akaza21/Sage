use super::*;
use crate::acp::bindings::codex::test_identity::{MANAGED_CODEX_HOME, user_agent};

use crate::{
    acp::{
        runtime::{JournalHandle, ProjectionMailbox, SqliteAdapterBatchJournal},
        store::{DatabaseAcpStore, load_chat_page},
    },
    persistence::Database,
};

#[test]
fn goal_active_response_mismatch_reaches_sqlite_only_after_client_verification() {
    runtime().block_on(async {
        let database = migrated_database_with_turn_state("sending");
        let mut codex = CodexRuntime::fixture_with_identity(
            Platform::WindowsX86_64,
            [7; 32],
            session("one"),
        );
        let init = codex.command(AdapterCommand::Initialize).unwrap();
        codex.resolve_egress(init.into_token(), EgressResolution::Queued).unwrap();
        let initialized = codex.ingest(&codex_frame(serde_json::json!({"id":1,"result":{
            "userAgent":user_agent("sage", "unknown"),
            "codexHome":MANAGED_CODEX_HOME,
            "platformFamily":"windows","platformOs":"windows"
        }})), &Mapper).unwrap();
        codex.resolve_egress(initialized.egress()[0].token().clone(), EgressResolution::Queued).unwrap();
        let start = codex.command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(), model: None,
        }).unwrap();
        codex.ingest(&codex_frame(serde_json::json!({
            "id":codex_request_id(&start),"result":{"thread":{"id":"external-session"}}
        })), &Mapper).unwrap();
        codex.enable_observed_goal_for_test();
        let prompt = codex.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: message(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        }).unwrap();
        let intent = session_intent().bind_turn(ActiveTurnIntent {
            turn_id: turn("one"), external_turn_id: None, client_message_id: message(),
        });
        let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let task = runner.spawn();
        let mut pump = AdapterPump::new(
            codex,
            SqliteAdapterBatchJournal::new(handle.clone(), intent.clone()),
            Mapper,
            Arc::new(ProjectionMailbox::default()),
            intent,
        ).unwrap();
        let stored = || database.connection().query_row(
            "SELECT state,revision FROM turns WHERE id='turn-one'", [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        ).unwrap();
        pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "id":codex_request_id(&prompt),"result":{"turn":{"id":"turn-A"}}
        })), 1).await.unwrap();
        assert_eq!(stored(), ("sending".into(), 0), "A must not commit Sending");
        pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "method":"turn/started","params":{"threadId":"external-session","turn":{"id":"turn-B"}}
        })), 2).await.unwrap();
        assert_eq!(stored(), ("sending".into(), 0), "B waits for client evidence");
        pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "method":"item/started","params":{"threadId":"external-session","turnId":"turn-B",
                "item":{"id":"echo-1","type":"userMessage","clientId":"message-one"}}
        })), 3).await.unwrap();
        assert_eq!(stored().0, "running");
        assert_eq!(pump.intent.active_turn.external_turn_id.as_ref().unwrap().as_str(), "turn-B");
        pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "method":"turn/completed","params":{"threadId":"external-session","turn":{"id":"turn-B","status":"completed"}}
        })), 4).await.unwrap();
        assert_eq!(stored().0, "completed");
        handle.close_when_drained();
        task.await.unwrap();
    });
}

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

pub(super) fn migrated_database() -> Database {
    migrated_database_with_turn_state("running")
}

fn migrated_database_with_turn_state(turn_state: &str) -> Database {
    migrated_database_with_session(turn_state, "external-session")
}

pub(super) fn migrated_database_with_session(
    turn_state: &str,
    external_session_id: &str,
) -> Database {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let connection = database.connection();
    connection.execute_batch(&format!(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Sign in',1,1,1);
         INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-one','project-one','C:/fixture','c:/fixture',0);
         INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Sign in',0,1,1);
         INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-one','project-one','chat-one',0,'binding','{digest}','{external_session_id}','{capabilities}','active','hot',0,0,0,1,1);
         INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1);
         INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-one','chat-one','session-one',1,'running','{artifact}',1);
         INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-one','project-one','chat-one','session-one',1,0,'intent-one','{turn_state}','{delivery_state}',0,1,1);
         INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,activity_id,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at) VALUES('item-one','project-one','chat-one','session-one','turn-one',1,0,'primary',NULL,'text','completed',NULL,'item-one','{{\"kind\":\"text\",\"payload\":{{\"text\":\"prompt\"}}}}',43,'b2bf66ef982a206f5552f31203f5825ec8155829efc5a4d0f63ba817a6dbe9fe',0,1,1,1);
         INSERT INTO acp_project_usage VALUES('project-one',1,1,1,0,0,0,0);
         INSERT INTO acp_session_usage VALUES('project-one','chat-one','session-one',1,0,0,0,0,0,0);
         UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=1 WHERE singleton=1;",
        digest = "07".repeat(32),
        artifact = "d".repeat(64),
        capabilities = serde_json::to_string(&pending_capabilities()).unwrap(),
        turn_state = turn_state,
        delivery_state = if turn_state == "sending" { "completeBytes" } else { "correlated" },
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
        prompt_resources: false,
        config_count: 0,
    }
}
