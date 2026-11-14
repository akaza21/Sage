use super::*;
use crate::acp::{projection::ItemContent, store::load_chat_page};

#[test]
fn named_and_unnamed_subagent_states_survive_shared_pump_and_sqlite_reopen() {
    runtime().block_on(async {
        let database = super::super::sign_in_tests::migrated_database();
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
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        }).unwrap();
        adapter.ingest(&codex_frame(serde_json::json!({
            "id":codex_request_id(&prompt),"result":{"turn":{"id":"turn-1"}}
        })), &Mapper).unwrap();
        adapter.ingest(&codex_frame(serde_json::json!({
            "method":"turn/started","params":{"threadId":"external-session","turn":{"id":"turn-1"}}
        })), &Mapper).unwrap();
        let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let task = runner.spawn();
        let mut pump = AdapterPump::new(adapter,
            SqliteAdapterBatchJournal::new(handle.clone(), intent.clone()), Mapper,
            Arc::new(ProjectionMailbox::default()), intent).unwrap();
        for (ordinal, item) in [
            serde_json::json!({"id":"named-start","type":"subAgentActivity",
                "agentThreadId":"child-a","agentPath":"Reviewer","kind":"started"}),
            serde_json::json!({"id":"mixed-wait","type":"collabAgentToolCall","status":"completed","tool":"wait",
                "receiverThreadIds":["child-a","child-b"],
                "agentsStates":{"child-a":{"status":"completed"},"child-b":{"status":"running"}}}),
        ].into_iter().enumerate() {
            let mut bytes = serde_json::to_vec(&serde_json::json!({"method":"item/completed",
                "params":{"threadId":"external-session","turnId":"turn-1","item":item}})).unwrap();
            bytes.push(b'\n');
            assert!(pump.push_stdout_chunk(&bytes, 10 + ordinal as i64).await.unwrap());
        }
        drop(pump);
        handle.close_when_drained();
        task.await.unwrap();
        let directory = std::env::temp_dir().join(format!("sage-subagent-identity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        {
            let source = database.connection();
            let mut copy = rusqlite::Connection::open(&path).unwrap();
            rusqlite::backup::Backup::new(&source, &mut copy).unwrap()
                .run_to_completion(256, std::time::Duration::ZERO, None).unwrap();
        }
        drop(database);
        let reopened = crate::acp::migration::open_or_migrate(&path).unwrap();
        let page = load_chat_page(&reopened, SageProjectId::parse("project-one").unwrap(),
            SageChatId::parse("chat-one").unwrap(), None).unwrap();
        let rows = page.turns.as_slice()[0].items.as_slice().iter()
            .filter_map(|item| match &item.content {
                ItemContent::Collaboration { label, operation, receiver_labels, agent_states, .. } => Some((
                    label.as_str(), operation.as_ref().map(SafeText::as_str),
                    receiver_labels.as_slice().iter().map(SafeText::as_str).collect::<Vec<_>>(),
                    agent_states.as_slice().iter().map(|(key, state)| (key.as_str(), state.as_str())).collect::<Vec<_>>())),
                _ => None,
            }).collect::<Vec<_>>();
        assert_eq!(rows, vec![
            ("Reviewer", Some("started"), vec![], vec![]),
            ("wait", Some("wait"), vec!["Reviewer", ""], vec![("child-a", "completed"), ("child-b", "running")]),
        ]);
        drop(page);
        drop(reopened);
        std::fs::remove_dir_all(directory).unwrap();
    });
}
