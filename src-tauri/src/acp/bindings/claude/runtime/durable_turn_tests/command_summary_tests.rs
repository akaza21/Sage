use super::*;
use crate::acp::{
    intent::CapabilitySnapshotV1,
    lifecycle::ToolState,
    projection::{ItemContent, ToolPresentation},
    store::load_chat_page,
};

fn command_fact(
    database: &Database,
    expected_capabilities: &CapabilitySnapshotV1,
) -> (ToolState, Option<String>) {
    let page = load_chat_page(
        &database.connection(),
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(
        &page.sessions.as_slice()[0].capabilities,
        expected_capabilities
    );
    let item = page.turns.as_slice()[0]
        .items
        .as_slice()
        .iter()
        .find(|item| matches!(item.content, ItemContent::ToolCall { .. }))
        .unwrap();
    let ItemContent::ToolCall {
        presentation,
        input_summary,
        ..
    } = &item.content
    else {
        panic!("command")
    };
    assert!(matches!(
        presentation,
        ToolPresentation::Execute { summary: None, .. }
    ));
    (
        item.tool_state.unwrap(),
        input_summary
            .as_ref()
            .map(|input| input.as_str().to_owned()),
    )
}

#[test]
fn claude_command_input_trace_reaches_live_and_settled_history_through_the_shared_pump() {
    for late_input in [false, true] {
        let mut runtime = ClaudeRuntime::fixture();
        ready(&mut runtime);
        let database = migrated_database(&runtime);
        let expected_capabilities = runtime.capabilities().expect("ready capabilities");
        let initial_page = load_chat_page(
            &database.connection(),
            SageProjectId::parse("project-one").unwrap(),
            SageChatId::parse("chat-one").unwrap(),
            None,
        )
        .expect("fixture capabilities must decode before command ingress");
        assert_eq!(
            initial_page.sessions.as_slice()[0].capabilities,
            expected_capabilities
        );
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let intent = session_intent().bind_turn(ActiveTurnIntent::store_minted(turn("one")).unwrap());
            let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
            let handle = CloseOnDrop(handle);
            let task = runner.spawn();
            let mut pump = AdapterPump::new(runtime, SqliteAdapterBatchJournal::new(handle.0.clone(), intent.clone()),
                Mapper, Arc::new(ProjectionMailbox::default()), intent).unwrap();
            pump.queue_prompt(Prompt::new("prompt", "Inspect the build command.").unwrap(),
                &RequiredText::new("clientMessageId", "turn-one").unwrap(),
                crate::settings_store::SessionAccessMode::ReadOnly).unwrap();
            let mut sink = Sink::default();
            pump.flush_one(&mut sink).unwrap();
            let request = sink.last_request_id.unwrap();
            let mut started = json!({"sessionUpdate": "tool_call", "toolCallId": "command-1",
                "kind": "execute", "status": "in_progress", "title": "Run command"});
            if !late_input {
                started["rawInput"] = json!({"command": "echo\n--token 'private value'"});
            }
            let mut bytes = serde_json::to_vec(&json!({"jsonrpc": "2.0", "method": "session/update",
                "params": {"sessionId": SESSION, "update": started}})).unwrap();
            bytes.push(b'\n');
            bounded("tool start", pump.push_stdout_chunk(&bytes, 10)).await.unwrap();
            assert_eq!(command_fact(&database, &expected_capabilities), (ToolState::InProgress,
                (!late_input).then(|| "echo --token [redacted]".to_owned())));
            if late_input {
                let mut bytes = serde_json::to_vec(&json!({"jsonrpc": "2.0", "method": "session/update",
                    "params": {"sessionId": SESSION, "update": {"sessionUpdate": "tool_call_update",
                        "toolCallId": "command-1", "rawInput": {"command": "echo\n--token 'private value'"}}}})).unwrap();
                bytes.push(b'\n');
                bounded("late input", pump.push_stdout_chunk(&bytes, 11)).await.unwrap();
                assert_eq!(command_fact(&database, &expected_capabilities), (ToolState::InProgress, Some("echo --token [redacted]".to_owned())));
            }
            for (offset, frame) in [
                json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": SESSION,
                    "update": {"sessionUpdate": "tool_call_update", "toolCallId": "command-1", "status": "completed"}}}),
                json!({"jsonrpc": "2.0", "id": request, "result": {"stopReason": "end_turn"}}),
            ].into_iter().enumerate() {
                let mut bytes = serde_json::to_vec(&frame).unwrap();
                bytes.push(b'\n');
                bounded("tool settles", pump.push_stdout_chunk(&bytes, 12 + offset as i64)).await.unwrap();
            }
            handle.0.close_when_drained();
            bounded("journal drain", task).await.unwrap();
        });
        assert_eq!(
            command_fact(&database, &expected_capabilities),
            (
                ToolState::Completed,
                Some("echo --token [redacted]".to_owned())
            )
        );
    }
}
