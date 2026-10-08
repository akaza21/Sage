use super::*;
use crate::acp::{
    bindings::codex::{CodexRuntime, Platform},
    runtime::{JournalHandle, ProjectionMailbox, SqliteAdapterBatchJournal},
    store::DatabaseAcpStore,
};

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
        idle.accept_idle_stdout_chunk(late_child.as_bytes(), 15).await.unwrap();
        let child_done = concat!(r#"{"method":"turn/completed","params":{"threadId":"child-native","turn":{"id":"native-turn-1","status":"completed"}}}"#, "\n");
        idle.accept_idle_stdout_chunk(child_done.as_bytes(), 16).await.unwrap();
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
        assert_eq!(snapshot.turns[0].items.len(), 4);
        assert_eq!(snapshot.turns[0].status, "completed");
        assert_eq!(snapshot.turns[0].items[3].id, "late-command");
        assert!(snapshot.turns[0].items.iter().all(|item| item.role.is_none()));
        assert!(matches!(snapshot.turns[0].items[0].payload,
            crate::acp::intent::NormalizedPayload::ToolCall { .. }));
        let mut read_wire: serde_json::Value = serde_json::from_str(include_str!(
            "fixtures/cp18d_child_read_0146.json"
        )).unwrap();
        read_wire["id"] = 41.into();
        read_wire["result"]["thread"]["id"] = "child-native".into();
        read_wire["result"]["thread"]["parentThreadId"] = "external-session".into();
        let read = crate::acp::bindings::codex::normalize_read_response(
            &serde_json::to_vec(&read_wire).unwrap(), 41, "child-native", "external-session", &Mapper
        ).unwrap();
        assert_eq!(read.turns[0].items.len(), 1);
        crate::acp::store::child_transcript::persist_snapshot(&mut reopened, &scope, &read, 11).unwrap();
        let merged = crate::acp::store::child_transcript::load_snapshot(&reopened, &scope)
            .unwrap().unwrap();
        assert_eq!(merged.turns.len(), 1);
        assert_eq!(merged.turns[0].items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            ["command-one", "file-one", "reason-one", "late-command", "item-1"]);
        assert_eq!(merged.turns.iter().map(|turn| turn.items.len()).sum::<usize>(), 5);
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
        assert_eq!(deduped.turns[0].items.len(), 5);
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
                "id":session_id,"result":{"thread":{"id":"external-session"}}
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
