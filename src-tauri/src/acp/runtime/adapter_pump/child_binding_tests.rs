use super::*;
use crate::acp::{
    bindings::codex::{CodexRuntime, Platform},
    runtime::{JournalHandle, ProjectionMailbox, SqliteAdapterBatchJournal},
    store::DatabaseAcpStore,
};

#[path = "child_binding_tests/subagent_identity_tests.rs"]
mod subagent_identity_tests;

#[cfg(feature = "acp-fixture")]
#[test]
fn generated_child_opened_journey_admits_owner_and_reads_after_parent_completion() {
    runtime().block_on(async {
        let frames = crate::acp::fixture::child_opened_wire_for_test(50);
        let parent = "fixture-persisted-session-1";
        let native_turn = frames.iter().find_map(|frame| frame.pointer("/result/turn/id")
            .and_then(serde_json::Value::as_str)).unwrap();
        let database = super::sign_in_tests::migrated_database();
        database.connection().execute("UPDATE agent_sessions SET external_session_id=?1 WHERE id='session-one'", [parent]).unwrap();
        let mut adapter = ready_codex_parent_for_thread(parent);
        adapter.enable_observed_goal_for_test();
        let prompt = adapter.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: message(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        }).unwrap();
        adapter.ingest(&codex_frame(serde_json::json!({"id":codex_request_id(&prompt),
            "result":{"turn":{"id":native_turn}}})), &Mapper).unwrap();
        let started = frames.iter().find(|frame| frame["method"] == "turn/started").unwrap();
        adapter.ingest(&codex_frame(started.clone()), &Mapper).unwrap();
        let mut session_scope = session_intent();
        session_scope.external_session_id = ExternalId::new("session", parent).unwrap();
        let intent = session_scope.bind_turn(ActiveTurnIntent { turn_id: turn("one"),
            external_turn_id: Some(ExternalId::new("turn", native_turn).unwrap()),
            client_message_id: message() });
        let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let task = runner.spawn();
        let mut pump = AdapterPump::new(adapter,
            SqliteAdapterBatchJournal::new(handle.clone(), intent.clone()), Mapper,
            Arc::new(ProjectionMailbox::default()), intent).unwrap();
        for frame in frames.iter().filter(|frame| frame["method"] == "item/completed" || frame["method"] == "turn/completed") {
            assert!(pump.push_stdout_chunk(&codex_line(frame.clone()), 9).await.unwrap());
        }
        let lifetime: (String, String, i64) = database.connection().query_row(
            "SELECT t.state,e.state,c.projection_revision FROM turns t
             JOIN session_runtime_epochs e ON e.session_id=t.session_id AND e.runtime_epoch=t.runtime_epoch
             JOIN chats c ON c.id=t.chat_id WHERE t.id='turn-one'", [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!((lifetime.0.as_str(), lifetime.1.as_str()), ("completed", "running"));
        assert!(lifetime.2 > 0);
        let item_id: String = database.connection().query_row(
            "SELECT item_id FROM acp_child_thread_bindings WHERE native_child_thread_id='fixture-child-1'", [], |row| row.get(0)).unwrap();
        let binding_source: String = database.connection().query_row(
            "SELECT external_item_id FROM items WHERE id=?1", [&item_id], |row| row.get(0)).unwrap();
        assert_eq!(binding_source, "fixture-child-activity-1");
        let observations: i64 = database.connection().query_row(
            "SELECT COUNT(*) FROM items WHERE turn_id='turn-one' AND kind='collaboration'", [], |row| row.get(0)).unwrap();
        assert_eq!(observations, 2);
        let scope = crate::acp::store::child_transcript::ChildTranscriptScope {
            owner: crate::acp::scheduler::RuntimeOwner {
                project_id: SageProjectId::parse("project-one").unwrap(), chat_id: SageChatId::parse("chat-one").unwrap(),
                session_id: session("one"), runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            }, item_id, child_thread_id: "fixture-child-1".into(), parent_thread_id: parent.into(),
        };
        crate::acp::store::child_transcript::verify_current_binding(&database.connection(), &scope).unwrap();
        let mut idle = pump.try_into_completed_parts().ok().expect("completed parent handoff");
        let mut output = CaptureTransport(Vec::new());
        idle.write_child(&mut output, "fixture-child-1", parent).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&output.0).unwrap();
        assert_eq!(request["method"], "thread/read");
        assert_eq!(request["params"]["includeTurns"], true);
        let replies = crate::acp::fixture::child_opened_wire_for_test(request["id"].as_u64().unwrap());
        let reply = replies.iter().find(|frame| frame.pointer("/result/thread/id")
            == Some(&serde_json::json!("fixture-child-1"))).unwrap();
        idle.accept_idle_stdout_chunk(&codex_line(reply.clone()), 10, &mut output).await.unwrap();
        let Some(crate::acp::goal::NativeRpcReply::ChildTranscript(snapshot)) = idle.take_native_rpc_reply() else { panic!("child read did not settle") };
        crate::acp::store::child_transcript::persist_snapshot(&mut database.connection(), &scope, &snapshot, 10).unwrap();
        assert_eq!(crate::acp::store::child_transcript::load_snapshot(&database.connection(), &scope).unwrap(), Some(snapshot));
        drop(idle);
        handle.close_when_drained();
        task.await.unwrap();
    });
}

#[test]
fn synthetic_child_command_survives_pump_store_and_reopen_without_parent_mutation() {
    runtime().block_on(async {
        let database = super::sign_in_tests::migrated_database();
        let mut adapter = ready_codex_parent();
        adapter.enable_observed_goal_for_test();
        let intent = session_intent().bind_turn(ActiveTurnIntent {
            turn_id: turn("one"),
            external_turn_id: Some(ExternalId::new("turn", "turn-1").unwrap()),
            client_message_id: message(),
        });
        let prompt = adapter.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: message(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        }).unwrap();
        let prompt_id = codex_request_id(&prompt);
        adapter.ingest(&codex_frame(serde_json::json!({
            "id":prompt_id,"result":{"turn":{"id":"turn-1"}}
        })), &Mapper).unwrap();
        adapter.ingest(&codex_frame(serde_json::json!({
            "method":"turn/started","params":{"threadId":"external-session","turn":{"id":"turn-1"}}
        })), &Mapper).unwrap();

        let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let task = runner.spawn();
        let mut pump = AdapterPump::new(
            adapter,
            SqliteAdapterBatchJournal::new(handle.clone(), intent.clone()),
            Mapper,
            Arc::new(ProjectionMailbox::default()),
            intent,
        ).unwrap();
        let wire = concat!(
            r#"{"method":"item/completed","params":{"threadId":"external-session","turnId":"turn-1","item":{"id":"activity-native","type":"subAgentActivity","agentThreadId":"child-native","agentPath":"Worker","kind":"completed"}}}"#,
            "\n",
        );
        assert!(pump.push_stdout_chunk(wire.as_bytes(), 9).await.unwrap());
        let started_wire = concat!(
            r#"{"method":"item/started","params":{"threadId":"child-native","turnId":"native-turn-1","item":{"id":"command-one","type":"commandExecution","status":"inProgress","command":"rg TODO src"}}}"#,
            "\n",
        );
        assert!(pump.push_stdout_chunk(started_wire.as_bytes(), 10).await.unwrap());
        let child_wire = concat!(
            r#"{"method":"item/completed","params":{"threadId":"child-native","turnId":"native-turn-1","item":{"id":"command-one","type":"commandExecution","status":"completed","command":"rg TODO src","aggregatedOutput":"done","exitCode":0}}}"#,
            "\n",
        );
        assert!(pump.push_stdout_chunk(child_wire.as_bytes(), 11).await.unwrap());
        let file_wire = concat!(
            r#"{"method":"item/completed","params":{"threadId":"child-native","turnId":"native-turn-1","item":{"id":"file-one","type":"fileChange","status":"completed","changes":[{"path":"C:/repo/a.ts","kind":{"type":"update"},"diff":"@@"}]}}}"#,
            "\n",
        );
        assert!(pump.push_stdout_chunk(file_wire.as_bytes(), 12).await.unwrap());
        let reason_wire = concat!(
            r#"{"method":"item/completed","params":{"threadId":"child-native","turnId":"native-turn-1","item":{"id":"reason-one","type":"reasoning","summary":["Need evidence"],"content":[]}}}"#,
            "\n",
        );
        assert!(pump.push_stdout_chunk(reason_wire.as_bytes(), 13).await.unwrap());
        let parent_done = concat!(r#"{"method":"turn/completed","params":{"threadId":"external-session","turn":{"id":"turn-1","status":"completed"}}}"#, "\n");
        assert!(pump.push_stdout_chunk(parent_done.as_bytes(), 14).await.unwrap());
        let mut idle = pump.try_into_completed_parts().ok().expect("terminal handoff");
        let late_child = concat!(r#"{"method":"item/completed","params":{"threadId":"child-native","turnId":"native-turn-1","item":{"id":"late-command","type":"commandExecution","status":"completed","command":"rg late","aggregatedOutput":"late","exitCode":0}}}"#, "\n");
        idle.accept_idle_stdout_chunk(late_child.as_bytes(), 15, &mut CaptureTransport(Vec::new())).await.unwrap();
        let child_agent = concat!(r#"{"method":"item/started","params":{"threadId":"child-native","turnId":"native-turn-1","item":{"id":"item-1","type":"agentMessage","text":"Live answer"}}}"#, "\n");
        idle.accept_idle_stdout_chunk(child_agent.as_bytes(), 16, &mut CaptureTransport(Vec::new())).await.unwrap();
        let child_user = concat!(r#"{"method":"item/completed","params":{"threadId":"child-native","turnId":"native-turn-1","item":{"id":"user-1","type":"userMessage","content":[{"type":"text","text":"Inspect this"}]}}}"#, "\n");
        idle.accept_idle_stdout_chunk(child_user.as_bytes(), 17, &mut CaptureTransport(Vec::new())).await.unwrap();
        let child_done = concat!(r#"{"method":"turn/completed","params":{"threadId":"child-native","turn":{"id":"native-turn-1","status":"completed"}}}"#, "\n");
        idle.accept_idle_stdout_chunk(child_done.as_bytes(), 18, &mut CaptureTransport(Vec::new())).await.unwrap();
        drop(idle);
        handle.close_when_drained();
        task.await.unwrap();

        let directory = std::env::temp_dir().join(format!("sage-codex-child-reopen-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        {
            let source = database.connection();
            let mut copy = rusqlite::Connection::open(&path).unwrap();
            rusqlite::backup::Backup::new(&source, &mut copy).unwrap()
                .run_to_completion(256, std::time::Duration::ZERO, None).unwrap();
        }
        drop(database);
        let mut reopened = crate::acp::migration::open_or_migrate(&path).unwrap();
        let stored: (String, String, String, String, String, i64) = reopened.query_row(
            "SELECT o.parent_thread_id,o.native_child_thread_id,b.session_id,b.turn_id,i.external_item_id,b.runtime_epoch
             FROM acp_child_thread_owners o JOIN acp_child_thread_bindings b
               ON b.native_child_thread_id=o.native_child_thread_id
             JOIN items i ON i.id=b.item_id",
            [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
        ).unwrap();
        assert_eq!(stored, (
            "external-session".into(), "child-native".into(), "session-one".into(),
            "turn-one".into(), "activity-native".into(), 1,
        ));
        let item_id: String = reopened.query_row(
            "SELECT item_id FROM acp_child_thread_bindings WHERE native_child_thread_id='child-native'",
            [], |row| row.get(0),
        ).unwrap();
        let scope = crate::acp::store::child_transcript::ChildTranscriptScope {
            owner: crate::acp::scheduler::RuntimeOwner {
                project_id: SageProjectId::parse("project-one").unwrap(),
                chat_id: SageChatId::parse("chat-one").unwrap(),
                session_id: session("one"),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            },
            item_id,
            child_thread_id: "child-native".into(),
            parent_thread_id: "external-session".into(),
        };
        let snapshot = crate::acp::store::child_transcript::load_snapshot(&reopened, &scope)
            .unwrap().expect("persisted synthetic child command");
        assert_eq!(snapshot.turns.len(), 1);
        assert_eq!(snapshot.turns[0].id, "native-turn-1");
        assert_eq!(snapshot.turns[0].items.len(), 6);
        assert_eq!(snapshot.turns[0].status, "completed");
        assert_eq!(snapshot.turns[0].items[3].id, "late-command");
        assert!(snapshot.turns[0].items[..4].iter().all(|item| item.role.is_none()));
        assert_eq!(snapshot.turns[0].items[4].role, Some(crate::acp::bindings::codex::ChildTextRole::Assistant));
        assert_eq!(snapshot.turns[0].items[5].role, Some(crate::acp::bindings::codex::ChildTextRole::User));
        assert!(matches!(snapshot.turns[0].items[0].payload,
            crate::acp::intent::NormalizedPayload::ToolCall { .. }));
        let mut read_wire: serde_json::Value = serde_json::from_str(include_str!(
            "fixtures/cp18d_child_read_0146.json"
        )).unwrap();
        read_wire["id"] = 41.into();
        read_wire["result"]["thread"]["id"] = "child-native".into();
        read_wire["result"]["thread"]["parentThreadId"] = "external-session".into();
        let mut read = crate::acp::bindings::codex::normalize_read_response(
            &serde_json::to_vec(&read_wire).unwrap(), 41, "child-native", "external-session", &Mapper
        ).unwrap();
        assert_eq!(read.turns[0].items.len(), 1);
        read.turns[0].items[0].role = None;
        crate::acp::store::child_transcript::persist_snapshot(&mut reopened, &scope, &read, 11).unwrap();
        let merged = crate::acp::store::child_transcript::load_snapshot(&reopened, &scope)
            .unwrap().unwrap();
        assert_eq!(merged.turns.len(), 1);
        assert_eq!(merged.turns[0].items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            ["command-one", "file-one", "reason-one", "late-command", "item-1", "user-1"]);
        assert_eq!(merged.turns.iter().map(|turn| turn.items.len()).sum::<usize>(), 6);
        assert_eq!(merged.turns[0].items[4].role, Some(crate::acp::bindings::codex::ChildTextRole::Assistant));
        assert!(merged.turns.iter().flat_map(|turn| &turn.items).any(|item|
            matches!(item.payload, crate::acp::intent::NormalizedPayload::ToolCall { .. })));
        assert!(merged.turns.iter().flat_map(|turn| &turn.items).any(|item|
            matches!(item.payload, crate::acp::intent::NormalizedPayload::Text { .. })));
        assert!(merged.turns.iter().flat_map(|turn| &turn.items).any(|item|
            matches!(item.payload, crate::acp::intent::NormalizedPayload::ThoughtSummary { .. })));
        assert!(merged.turns.iter().flat_map(|turn| &turn.items).any(|item|
            matches!(item.payload, crate::acp::intent::NormalizedPayload::FileChangeDiff { .. })));
        read_wire["result"]["thread"]["turns"][0]["items"].as_array_mut().unwrap().push(
            serde_json::json!({"id":"command-one","type":"commandExecution",
                "status":"inProgress","command":"rg TODO src"})
        );
        let overlapping_read = crate::acp::bindings::codex::normalize_read_response(
            &serde_json::to_vec(&read_wire).unwrap(), 41, "child-native", "external-session", &Mapper
        ).unwrap();
        crate::acp::store::child_transcript::persist_snapshot(&mut reopened, &scope, &overlapping_read, 12).unwrap();
        let deduped = crate::acp::store::child_transcript::load_snapshot(&reopened, &scope)
            .unwrap().unwrap();
        assert_eq!(deduped.turns[0].items.len(), 6);
        assert!(matches!(deduped.turns[0].items[1].lifecycle, ItemLifecycle::Completed));
        let parent_items: i64 = reopened.query_row(
            "SELECT COUNT(*) FROM items WHERE external_item_id IN ('activity-native','command-one')", [], |row| row.get(0)
        ).unwrap();
        assert_eq!(parent_items, 1);
        reopened.execute("UPDATE session_runtime_epochs SET state='exited',terminal_at=13,exit_kind='processExit' WHERE session_id='session-one' AND runtime_epoch=1", []).unwrap();
        assert_eq!(crate::acp::store::child_transcript::load_earlier_snapshot(&reopened, &scope).unwrap(), Some(deduped));
        drop(reopened);
        std::fs::remove_dir_all(directory).unwrap();
    });
}

fn ready_codex_parent() -> CodexRuntime {
    ready_codex_parent_for_thread("external-session")
}

fn ready_codex_parent_for_thread(parent: &str) -> CodexRuntime {
    let mut adapter =
        CodexRuntime::fixture_with_identity(Platform::WindowsX86_64, [7; 32], session("one"));
    let initialize = adapter.command(AdapterCommand::Initialize).unwrap();
    let initialize_id = codex_request_id(&initialize);
    adapter
        .resolve_egress(initialize.into_token(), EgressResolution::Queued)
        .unwrap();
    let initialized = adapter
        .ingest(
            &codex_frame(serde_json::json!({
                "id":initialize_id,"result":{
                    "userAgent":"sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
                    "codexHome":"C:\\SageHarnesses\\CodexState\\codex-app-server-0.146.0-d3992fec",
                    "platformFamily":"windows","platformOs":"windows"
                }
            })),
            &Mapper,
        )
        .unwrap();
    adapter
        .resolve_egress(
            initialized.egress()[0].token().clone(),
            EgressResolution::Queued,
        )
        .unwrap();
    let session = adapter
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    let session_id = codex_request_id(&session);
    adapter
        .ingest(
            &codex_frame(serde_json::json!({
                "id":session_id,"result":{"thread":{"id":parent}}
            })),
            &Mapper,
        )
        .unwrap();
    adapter
}

fn codex_request_id(egress: &AdapterEgress) -> u64 {
    serde_json::from_slice::<serde_json::Value>(egress.frame().payload()).unwrap()["id"]
        .as_u64()
        .unwrap()
}

fn codex_frame(value: serde_json::Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    IncrementalFrameDecoder::new()
        .push_chunk(&bytes)
        .unwrap()
        .unwrap()
}
