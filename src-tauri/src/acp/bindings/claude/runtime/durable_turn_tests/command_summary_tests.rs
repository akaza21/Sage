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
) -> (ToolState, Option<String>, Option<String>) {
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
        output_summary,
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
        output_summary
            .as_ref()
            .map(|output| output.as_str().to_owned()),
    )
}

#[test]
fn claude_long_command_and_raw_or_console_output_survive_the_pump_and_sqlite_reopen() {
    for (output, expected_output) in [
        ("ok --token private-value", "ok --token [redacted]"),
        ("ok --token 'private value'", "ok --token [redacted]"),
        ("ok password: hunter2", "ok password: [redacted]"),
        (r#"ok "api_key": "abc123""#, r#"ok "api_key": [redacted]"#),
        (r#"curl -H "X-Api-Key: abc123""#, "curl -H [redacted]"),
        ("curl -d @/home/alice/secret.json", "curl -d [redacted]"),
        (
            "cc -L/home/alice/lib -I/home/alice/inc",
            "cc [redacted] [redacted]",
        ),
    ] {
        for (late_input, raw_output) in [(false, false), (false, true), (true, false), (true, true)]
        {
            let directory = HistoryDirectory(
                std::env::temp_dir().join(format!("sage-command-{}", uuid::Uuid::new_v4())),
            );
            std::fs::create_dir(&directory.0).unwrap();
            let path = directory.0.join("sage.db");
            let mut runtime = ClaudeRuntime::fixture();
            ready(&mut runtime);
            let fixture = migrated_database(&runtime);
            {
                let source = fixture.connection();
                let mut copy = rusqlite::Connection::open(&path).unwrap();
                rusqlite::backup::Backup::new(&source, &mut copy)
                    .unwrap()
                    .run_to_completion(256, std::time::Duration::ZERO, None)
                    .unwrap();
            }
            drop(fixture);
            let database = Database::open(&path).unwrap();
            let command = format!("echo {} --token 'private value'", "x".repeat(1_000));
            let retained = format!("echo {} --token [redacted]", "x".repeat(1_000));
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
                crate::acp_protocol::access_mode::SessionAccessMode::Ask).unwrap();
            let mut sink = Sink::default();
            pump.flush_one(&mut sink).unwrap();
            let request = sink.last_request_id.unwrap();
            let mut started = json!({"sessionUpdate": "tool_call", "toolCallId": "command-1",
                "kind": "execute", "status": "in_progress", "title": "Run command"});
            if !late_input {
                started["rawInput"] = json!({"command": command});
            }
            let mut bytes = serde_json::to_vec(&json!({"jsonrpc": "2.0", "method": "session/update",
                "params": {"sessionId": SESSION, "update": started}})).unwrap();
            bytes.push(b'\n');
            bounded("tool start", pump.push_stdout_chunk(&bytes, 10)).await.unwrap();
            assert_eq!(command_fact(&database, &expected_capabilities), (ToolState::InProgress,
                (!late_input).then(|| retained.clone()), None));
            if late_input {
                let mut bytes = serde_json::to_vec(&json!({"jsonrpc": "2.0", "method": "session/update",
                    "params": {"sessionId": SESSION, "update": {"sessionUpdate": "tool_call_update",
                        "toolCallId": "command-1", "rawInput": {"command": command}}}})).unwrap();
                bytes.push(b'\n');
                bounded("late input", pump.push_stdout_chunk(&bytes, 11)).await.unwrap();
                assert_eq!(command_fact(&database, &expected_capabilities), (ToolState::InProgress, Some(retained.clone()), None));
            }
            let console_output = if raw_output { "console fallback must not win" } else { output };
            let mut completed = json!({"sessionUpdate": "tool_call_update", "toolCallId": "command-1", "status": "completed",
                "content": [{"type":"content","content":{"type":"text","text":format!("```console\n{console_output}\n```")}}]});
            if raw_output {
                completed["rawOutput"] = json!(output);
            }
            for (offset, frame) in [
                json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": SESSION, "update": completed}}),
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
                    Some(retained.clone()),
                    Some(expected_output.to_owned())
                )
            );
            drop(database);
            let reopened = Database::open(&path).unwrap();
            assert_eq!(
                command_fact(&reopened, &expected_capabilities),
                (
                    ToolState::Completed,
                    Some(retained),
                    Some(expected_output.to_owned())
                )
            );
        }
    }
}

struct HistoryDirectory(std::path::PathBuf);

impl Drop for HistoryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
