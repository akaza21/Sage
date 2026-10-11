use super::*;

use crate::{
    acp::{
        bindings::adapter::{AdapterCommand, EgressResolution, HarnessAdapter},
        contract::{SageChatId, SageProjectId, SageTurnId},
        intent::Prompt,
        runtime::{
            ActiveTurnIntent, AdapterPump, EgressTransport, JournalHandle, ProjectionMailbox,
            SessionPumpIntent, SqliteAdapterBatchJournal,
        },
        store::DatabaseAcpStore,
        transport::FrameWriteOutcome,
    },
    persistence::Database,
};

use super::tests::{Mapper, frame, initialize, request_id};

mod command_summary_tests;
mod task_lifecycle_tests;

#[derive(Default)]
struct Sink {
    last_request_id: Option<u64>,
}

impl EgressTransport for Sink {
    fn write(&mut self, frame: &crate::acp::transport::OutboundFrame) -> FrameWriteOutcome {
        self.last_request_id = serde_json::from_slice::<Value>(frame.payload())
            .ok()
            .and_then(|payload| payload["id"].as_u64());
        FrameWriteOutcome::Complete
    }
}

const SESSION: &str = "claude-session-1";
const SAGE_SESSION: &str = "session-claude-runtime";

const STEP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

async fn bounded<F: std::future::Future>(label: &str, future: F) -> F::Output {
    match tokio::time::timeout(STEP_TIMEOUT, future).await {
        Ok(value) => value,
        Err(_) => panic!("{label} did not settle within {STEP_TIMEOUT:?}"),
    }
}

struct CloseOnDrop(JournalHandle);

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        self.0.close_when_drained();
    }
}

#[test]
fn two_prompts_in_one_epoch_persist_both_agent_messages_and_both_usage_snapshots() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let database = migrated_database(&runtime);
    let session_intent = session_intent();

    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(async {
            let intent = session_intent
                .clone()
                .bind_turn(ActiveTurnIntent::store_minted(turn("one")).expect("turn one intent"));
            let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
            let handle = CloseOnDrop(handle);
            let task = runner.spawn();
            let mut pump = AdapterPump::new(
                runtime,
                SqliteAdapterBatchJournal::new(handle.0.clone(), intent.clone()),
                Mapper,
                Arc::new(ProjectionMailbox::default()),
                intent,
            )
            .expect("first pump");

            let first = drive_turn(&mut pump, "turn-one", "ok", 10).await;
            assert_eq!(first, 3);
            let (state, items) = turn_facts(&database, "turn-one");
            assert!(
                pump.completed_turn_ready(),
                "turn-one not quiescent: state={state} items={items} teardown={} cursor={}",
                pump.teardown_required(),
                pump.latest_durable_turn_cursor().is_some(),
            );
            let parts = pump
                .try_into_completed_parts()
                .map_err(|boxed| boxed.1)
                .expect("first turn handed the session back");
            mint_second_turn(&database);
            let mut pump = parts
                .bind_turn(ActiveTurnIntent::store_minted(turn("two")).expect("turn two intent"))
                .map_err(|boxed| boxed.1)
                .expect("second turn bound to the same session and epoch");

            let second = drive_turn(&mut pump, "turn-two", "ok again", 20).await;
            assert_eq!(second, 4);
            assert!(pump.completed_turn_ready());

            handle.0.close_when_drained();
            bounded("journal drain", task)
                .await
                .expect("journal drained");
        });

    let connection = database.connection();
    let mut statement = connection
        .prepare("SELECT turn_id,external_item_id FROM items WHERE kind='text' AND external_item_id LIKE 'agent-message%' ORDER BY turn_id")
        .expect("item query");
    let messages: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("item rows")
        .map(|row| row.expect("item row"))
        .collect();
    assert_eq!(
        messages,
        [
            ("turn-one".to_owned(), "agent-message:3".to_owned()),
            ("turn-two".to_owned(), "agent-message:4".to_owned()),
        ]
    );

    let mut statement = connection
        .prepare("SELECT turn_id,input_tokens,output_tokens,context_window FROM acp_turn_usage_snapshots ORDER BY turn_id")
        .expect("usage query");
    let usage: Vec<(String, i64, i64, Option<i64>)> = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("usage rows")
        .map(|row| row.expect("usage row"))
        .collect();
    assert_eq!(
        usage,
        [
            ("turn-one".to_owned(), 2, 4, Some(1_000_000)),
            ("turn-two".to_owned(), 2, 4, Some(1_000_000)),
        ]
    );

    let mut statement = connection
        .prepare("SELECT id,state FROM turns ORDER BY ordinal")
        .expect("turn query");
    let turns: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("turn rows")
        .map(|row| row.expect("turn row"))
        .collect();
    assert_eq!(
        turns,
        [
            ("turn-one".to_owned(), "completed".to_owned()),
            ("turn-two".to_owned(), "completed".to_owned()),
        ]
    );
}

async fn drive_turn<J: crate::acp::runtime::AdapterBatchJournal>(
    pump: &mut AdapterPump<ClaudeRuntime, J, Mapper>,
    turn_id: &str,
    text: &str,
    now_millis: i64,
) -> u64 {
    pump.queue_prompt(
        Prompt::new("prompt", "Reply with the single word ok.").expect("prompt"),
        &RequiredText::new("clientMessageId", turn_id.to_owned()).expect("client message id"),
        crate::settings_store::SessionAccessMode::ReadOnly,
    )
    .expect("prompt queued");
    let mut sink = Sink::default();
    assert_eq!(
        pump.flush_one(&mut sink).expect("prompt flushed"),
        crate::acp::runtime::FlushDisposition::Written
    );
    let request = sink
        .last_request_id
        .expect("outbound prompt carried a request id");

    for (offset, chunk) in [
        format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{SESSION}","update":{{"sessionUpdate":"usage_update","used":23753,"size":1000000}}}}}}"#
        ),
        format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{SESSION}","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{text}"}},"messageId":"msg-{request}"}}}}}}"#
        ),
        format!(
            r#"{{"jsonrpc":"2.0","id":{request},"result":{{"stopReason":"end_turn","usage":{{"inputTokens":2,"outputTokens":4,"cachedReadTokens":23747,"cachedWriteTokens":0,"totalTokens":23753}}}}}}"#
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut bytes = chunk.into_bytes();
        bytes.push(b'\n');
        let accepted = bounded(
            &format!("{turn_id} stdout chunk {offset}"),
            pump.push_stdout_chunk(&bytes, now_millis + offset as i64),
        )
        .await
        .expect("stdout chunk accepted");
        assert!(accepted, "{turn_id} chunk {offset} produced no frame");
    }
    request
}

fn ready(runtime: &mut ClaudeRuntime) {
    initialize(runtime);
    let egress = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: None,
        })
        .expect("new session");
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .expect("written");
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":SESSION}})),
            &Mapper,
        )
        .expect("session response");
}

fn turn(suffix: &str) -> SageTurnId {
    SageTurnId::parse(format!("turn-{suffix}")).expect("turn id")
}

fn mint_second_turn(database: &Database) {
    let connection = database.connection();
    connection
        .execute_batch(
            &format!(
                "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-two','project-one','chat-one','{SAGE_SESSION}',3,1,'intent-two','sending','completeBytes',0,1,1);"
            ),
        )
        .expect("mint second turn");
    drop(connection);
}

fn turn_facts(database: &Database, turn_id: &str) -> (String, i64) {
    let connection = database.connection();
    let facts = connection
        .query_row(
            "SELECT state,(SELECT COUNT(*) FROM items WHERE turn_id=?1) FROM turns WHERE id=?1",
            [turn_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("turn facts");
    drop(connection);
    facts
}

fn session_intent() -> SessionPumpIntent {
    SessionPumpIntent {
        binding_digest: [7; 32],
        project_id: SageProjectId::parse("project-one").expect("project"),
        chat_id: SageChatId::parse("chat-one").expect("chat"),
        session_id: SageAgentSessionId::parse(SAGE_SESSION).expect("session"),
        external_session_id: ExternalId::new("externalSessionId", SESSION).expect("external"),
        runtime_epoch: RuntimeEpoch::new(3).expect("epoch"),
    }
}

fn migrated_database(runtime: &ClaudeRuntime) -> Database {
    let capabilities = serde_json::to_string(&runtime.capabilities().expect("ready capabilities"))
        .expect("serialize capabilities");
    let database = Database::open(std::path::Path::new(":memory:")).expect("database");
    let connection = database.connection();
    connection
        .execute_batch(&format!(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Two turns',1,1,1);
             INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-one','project-one','C:/fixture/root','c:/fixture/root',0);
             INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Two turns',0,1,1);
             INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('{SAGE_SESSION}','project-one','chat-one',0,'claude-agent-acp','{digest}','{SESSION}','{capabilities}','active','hot',0,0,0,1,1);
             INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','{SAGE_SESSION}',0,1);
             INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-one','chat-one','{SAGE_SESSION}',3,'running','{artifact}',1);
             INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-one','project-one','chat-one','{SAGE_SESSION}',3,0,'intent-one','sending','completeBytes',0,1,1);
             INSERT INTO acp_project_usage VALUES('project-one',1,1,2,0,0,0,0);
             INSERT INTO acp_session_usage VALUES('project-one','chat-one','{SAGE_SESSION}',2,0,0,0,0,0,0);
             UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=2 WHERE singleton=1;",
            digest = "07".repeat(32),
            artifact = "d".repeat(64),
        ))
        .expect("seed");
    drop(connection);
    database
}
