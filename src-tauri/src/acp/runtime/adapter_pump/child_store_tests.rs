use super::*;
use crate::acp::{
    goal::NativeRpcReply,
    scheduler::RuntimeOwner,
    store::child_transcript::{
        ChildTranscriptScope, load_earlier_snapshot, load_snapshot, persist_snapshot,
    },
};

/// The source fixture is the redacted second 0.146 probe, whose parent read
/// proved the subAgentActivity linkage. Only its synthetic request/parent IDs
/// are rebound to this pump's test owner.
fn observed_child_wire(request_id: u64) -> Vec<u8> {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/cp18d_child_read_0146.json")).unwrap();
    value["id"] = request_id.into();
    value["result"]["thread"]["parentThreadId"] = "thread-1".into();
    codex_line(value)
}

fn sqlite_child_owner(path: &std::path::Path) -> rusqlite::Connection {
    let (connection, _) = crate::acp::migration::open_or_migrate(path)
        .unwrap()
        .into_parts();
    connection.execute_batch(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Project',1,1,1);
         INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-one','project-one','C:/project','c:/project',0);
         INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Chat',0,1,1);
         INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-one','project-one','chat-one',0,'binding','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','thread-1','{}','active','hot',0,0,0,1,1);
         INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1);
         INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-one','chat-one','session-one',1,'running','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',1);
         INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-one','project-one','chat-one','session-one',1,0,'intent-one','running','correlated',0,1,1);
         INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,activity_id,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at) VALUES('item-one','project-one','chat-one','session-one','turn-one',1,0,'primary',NULL,'collaboration','started',NULL,'activity-one','{}',2,'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',0,1,1,NULL);
         INSERT INTO acp_child_thread_owners(native_child_thread_id,project_id,chat_id,session_id,runtime_epoch,parent_thread_id,source,created_at) VALUES('child-one','project-one','chat-one','session-one',1,'thread-1','codexSubAgentActivity',1);
         INSERT INTO acp_child_thread_bindings(item_id,project_id,chat_id,session_id,turn_id,runtime_epoch,native_child_thread_id,created_at) VALUES('item-one','project-one','chat-one','session-one','turn-one',1,'child-one',1);",
    ).unwrap();
    connection
}

fn scope() -> ChildTranscriptScope {
    ChildTranscriptScope {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-one").unwrap(),
            chat_id: SageChatId::parse("chat-one").unwrap(),
            session_id: SageAgentSessionId::parse("session-one").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        },
        item_id: "item-one".into(),
        child_thread_id: "child-one".into(),
        parent_thread_id: "thread-1".into(),
    }
}

#[test]
fn observed_wire_pump_snapshot_survives_reopen_without_mutating_parent_turn() {
    runtime().block_on(async {
        let (mut pump, _, _) = goal_active_codex_pump();
        pump.adapter.learn_child_for_test("child-one");
        let mut output = CaptureTransport(Vec::new());
        pump.write_child(&mut output, "child-one", "thread-1").unwrap();
        let request_id = serde_json::from_slice::<serde_json::Value>(&output.0).unwrap()["id"]
            .as_u64()
            .unwrap();
        assert!(pump.push_stdout_chunk(&observed_child_wire(request_id), 10).await.unwrap());
        let Some(NativeRpcReply::ChildTranscript(snapshot)) = pump.take_native_rpc_reply() else {
            panic!("the observed wire did not yield a typed child transcript");
        };
        assert_eq!(snapshot.turns.len(), 1);
        assert_eq!(snapshot.turns[0].items.len(), 1);
        assert!(matches!(snapshot.turns[0].items[0].payload, NormalizedPayload::Text { .. }));

        let directory = std::env::temp_dir().join(format!("sage-cp18d-wire-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        let mut connection = sqlite_child_owner(&path);
        let parent_before: (String, i64, i64) = connection.query_row(
            "SELECT state,revision,(SELECT COUNT(*) FROM items WHERE turn_id='turn-one') FROM turns WHERE id='turn-one'",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        let scope = scope();
        connection.execute("DELETE FROM chat_session_heads WHERE chat_id='chat-one'", []).unwrap();
        assert!(persist_snapshot(&mut connection, &scope, &snapshot, 9).is_err(),
            "a session-head transition between native reply and store commit must reject the write");
        connection.execute("INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1)", []).unwrap();
        persist_snapshot(&mut connection, &scope, &snapshot, 10).unwrap();
        let mut foreign = scope.clone();
        foreign.parent_thread_id = "other-parent".into();
        assert!(persist_snapshot(&mut connection, &foreign, &snapshot, 11).is_err());
        drop(connection);

        let (mut reopened, _) = crate::acp::migration::open_or_migrate(&path).unwrap().into_parts();
        assert_eq!(load_snapshot(&reopened, &scope).unwrap(), Some(snapshot.clone()));
        let parent_after: (String, i64, i64) = reopened.query_row(
            "SELECT state,revision,(SELECT COUNT(*) FROM items WHERE turn_id='turn-one') FROM turns WHERE id='turn-one'",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(parent_before, parent_after);
        reopened.execute("UPDATE session_runtime_epochs SET state='terminating' WHERE session_id='session-one' AND runtime_epoch=1", []).unwrap();
        assert!(persist_snapshot(&mut reopened, &scope, &snapshot, 12).is_err(),
            "an epoch transition after native reply must reject a later write");
        reopened.execute("UPDATE session_runtime_epochs SET state='exited',terminal_at=12,exit_kind='processExit' WHERE session_id='session-one' AND runtime_epoch=1", []).unwrap();
        assert!(persist_snapshot(&mut reopened, &scope, &snapshot, 13).is_err());
        drop(reopened);
        let (restarted, _) = crate::acp::migration::open_or_migrate(&path).unwrap().into_parts();
        assert_eq!(load_earlier_snapshot(&restarted, &scope).unwrap(), Some(snapshot));
        assert!(load_earlier_snapshot(&restarted, &foreign).unwrap().is_none());
        restarted.execute("DELETE FROM chat_session_heads WHERE chat_id='chat-one'", []).unwrap();
        assert!(load_earlier_snapshot(&restarted, &scope).unwrap().is_none());
        drop(restarted);
        std::fs::remove_dir_all(directory).unwrap();
    });
}
