use super::*;
use crate::acp::bindings::codex::test_identity::{MANAGED_CODEX_HOME, user_agent};
use crate::{
    acp::{
        projection::ItemContent,
        runtime::{JournalHandle, ProjectionMailbox, SqliteAdapterBatchJournal},
        store::{DatabaseAcpStore, load_chat_page},
    },
    persistence::Database,
};

struct TempRoot(std::path::PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("sage-cp28-terminal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct JournalShutdown(JournalHandle);

impl Drop for JournalShutdown {
    fn drop(&mut self) {
        self.0.close_when_drained();
    }
}

#[test]
fn codex_terminal_only_error_survives_the_shared_pump_history_and_database_reopen() {
    runtime().block_on(async {
        let root = TempRoot::new();
        let path = root.0.join("sage.db");
        let fixture = sign_in_tests::migrated_database();
        {
            let source = fixture.connection();
            let mut copy = rusqlite::Connection::open(&path).unwrap();
            rusqlite::backup::Backup::new(&source, &mut copy).unwrap()
                .run_to_completion(256, std::time::Duration::ZERO, None).unwrap();
        }
        drop(fixture);
        let database = Database::open(&path).unwrap();
        let mut codex = CodexRuntime::fixture_with_identity(Platform::WindowsX86_64, [7; 32], session("one"));
        let init = codex.command(AdapterCommand::Initialize).unwrap();
        codex.resolve_egress(init.into_token(), EgressResolution::Queued).unwrap();
        let initialized = codex.ingest(&codex_frame(serde_json::json!({"id":1,"result":{
            "userAgent":user_agent("sage", "unknown"),
            "codexHome":MANAGED_CODEX_HOME,
            "platformFamily":"windows","platformOs":"windows"
        }})), &Mapper).unwrap();
        codex.resolve_egress(initialized.egress()[0].token().clone(), EgressResolution::Queued).unwrap();
        let start = codex.command(AdapterCommand::NewSession { native_root: r"C:\repo".into(), model: None }).unwrap();
        codex.ingest(&codex_frame(serde_json::json!({"id":codex_request_id(&start),"result":{"thread":{"id":"external-session"}}})), &Mapper).unwrap();
        codex.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(), client_message_id: message(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        }).unwrap();
        codex.ingest(&codex_frame(serde_json::json!({"method":"turn/started","params":{
            "threadId":"external-session","turn":{"id":"external-turn","status":"inProgress"}
        }})), &Mapper).unwrap();
        let intent = session_intent().bind_turn(ActiveTurnIntent {
            turn_id: turn("one"), external_turn_id: Some(ExternalId::new("turn", "external-turn").unwrap()), client_message_id: message(),
        });
        let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let shutdown = JournalShutdown(handle.clone());
        let task = runner.spawn();
        let mut pump = AdapterPump::new(codex, SqliteAdapterBatchJournal::new(handle.clone(), intent.clone()), Mapper, Arc::new(ProjectionMailbox::default()), intent).unwrap();
        let frame = codex_line(serde_json::json!({"method":"turn/completed","params":{
            "threadId":"external-session","turn":{"id":"external-turn","status":"failed",
            "error":{"message":"Failed with sk-private at C:\\private\\file.txt"}}
        }}));
        pump.push_stdout_chunk(&frame, 10).await.unwrap();
        assert!(pump.completed_turn_ready());
        assert_failed_history(&database);
        pump.push_stdout_chunk(&frame, 11).await.unwrap();
        assert_failed_history(&database);
        drop(pump);
        handle.close_when_drained();
        task.await.unwrap();
        drop(shutdown);
        drop(handle);
        drop(database);
        let reopened = Database::open(&path).unwrap();
        assert_failed_history(&reopened);
        drop(reopened);
    });
}

fn assert_failed_history(database: &Database) {
    let connection = database.connection();
    let page = load_chat_page(
        &connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(page.turns.as_slice().len(), 1);
    let turn = &page.turns.as_slice()[0];
    assert_eq!(turn.state, TurnState::Failed);
    let errors = turn
        .items
        .as_slice()
        .iter()
        .filter_map(|item| match &item.content {
            ItemContent::Error {
                code,
                message,
                recoverable,
                ..
            } => Some((code.as_str(), message.as_str(), *recoverable)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        errors,
        [("codex", "Failed with [redacted] at [path]", false)]
    );
}
