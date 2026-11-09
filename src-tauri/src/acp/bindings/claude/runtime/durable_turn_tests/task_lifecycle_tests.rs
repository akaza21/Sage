use super::*;
use crate::acp::{
    lifecycle::ToolState,
    projection::{ItemContent, ToolPresentation},
    store::load_chat_page,
};

fn task_facts(connection: &rusqlite::Connection) -> Vec<(ToolState, String, Option<bool>)> {
    let page = load_chat_page(
        connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    assert!(
        !page.sessions.as_slice()[0]
            .capabilities
            .child_transcript_read
    );
    page.turns.as_slice()[0]
        .items
        .as_slice()
        .iter()
        .filter_map(|item| match &item.content {
            ItemContent::ToolCall {
                presentation:
                    ToolPresentation::Subagent {
                        label,
                        task_lifecycle,
                        ..
                    },
                ..
            } => Some((
                item.tool_state.unwrap(),
                label.as_str().to_owned(),
                *task_lifecycle,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn claude_task_metadata_and_tool_lifecycle_survive_pump_and_reopen_without_child_read_capability() {
    let mut adapter = ClaudeRuntime::fixture();
    ready(&mut adapter);
    let database = migrated_database(&adapter);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let intent = session_intent().bind_turn(ActiveTurnIntent::store_minted(turn("one")).unwrap());
        let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let handle = CloseOnDrop(handle);
        let task = runner.spawn();
        let mut pump = AdapterPump::new(adapter, SqliteAdapterBatchJournal::new(handle.0.clone(), intent.clone()),
            Mapper, Arc::new(ProjectionMailbox::default()), intent).unwrap();
        pump.queue_prompt(Prompt::new("prompt", "Review accessibility.").unwrap(),
            &RequiredText::new("clientMessageId", "turn-one").unwrap(),
            crate::settings_store::SessionAccessMode::Ask).unwrap();
        let mut sink = Sink::default();
        pump.flush_one(&mut sink).unwrap();
        let request = sink.last_request_id.unwrap();
        for (ordinal, (update, expected)) in [
            (json!({"sessionUpdate":"tool_call","toolCallId":"task-1","kind":"other","status":"in_progress",
                "title":"Review accessibility","_meta":{"claudeCode":{"toolName":"Task"}}}), ToolState::InProgress),
            (json!({"sessionUpdate":"tool_call_update","toolCallId":"task-1","status":"completed"}), ToolState::Completed),
        ].into_iter().enumerate() {
            let mut bytes = serde_json::to_vec(&json!({"jsonrpc":"2.0","method":"session/update",
                "params":{"sessionId":SESSION,"update":update}})).unwrap();
            bytes.push(b'\n');
            bounded("task update", pump.push_stdout_chunk(&bytes, 10 + ordinal as i64)).await.unwrap();
            assert_eq!(task_facts(&database.connection()), vec![(expected, "Review accessibility".to_owned(), Some(true))]);
        }
        let mut bytes = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":request,"result":{"stopReason":"end_turn"}})).unwrap();
        bytes.push(b'\n');
        bounded("task turn settles", pump.push_stdout_chunk(&bytes, 12)).await.unwrap();
        handle.0.close_when_drained();
        bounded("task journal drains", task).await.unwrap();
    });
    let directory = std::env::temp_dir().join(format!("sage-claude-task-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    {
        let source = database.connection();
        let mut copy = rusqlite::Connection::open(&path).unwrap();
        rusqlite::backup::Backup::new(&source, &mut copy)
            .unwrap()
            .run_to_completion(256, std::time::Duration::ZERO, None)
            .unwrap();
    }
    drop(database);
    let reopened = crate::acp::migration::open_or_migrate(&path).unwrap();
    assert_eq!(
        task_facts(&reopened),
        vec![(
            ToolState::Completed,
            "Review accessibility".to_owned(),
            Some(true)
        )]
    );
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}
