use super::*;
use crate::acp::{
    goal::NativeRpcReply,
    scheduler::RuntimeOwner,
    store::child_transcript::{
        ChildTranscriptScope, load_earlier_snapshot, load_snapshot, persist_snapshot,
    },
};

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

#[test]
fn synthetic_child_item_flood_marks_retention_limit_without_dropping_admitted_items() {
    use crate::acp::{
        bindings::codex::{ChildItem, ChildTranscriptSnapshot, ChildTurn},
        lifecycle::ItemLifecycle,
        store::{AdapterStoreOwner, ChildLiveItemCommand},
    };

    let directory = std::env::temp_dir().join(format!("sage-cp18e-cap-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let mut connection = sqlite_child_owner(&path);
    let scope = scope();
    let text = SafeText::new("text", "retained").unwrap();
    let snapshot = ChildTranscriptSnapshot {
        child_thread_id: "child-one".into(),
        parent_thread_id: "thread-1".into(),
        turns: vec![ChildTurn {
            id: "child-turn-1".into(),
            status: "inProgress".into(),
            items: (0..1024)
                .map(|index| ChildItem {
                    id: format!("item-{index}"),
                    role: None,
                    observed_sequence: None,
                    lifecycle: ItemLifecycle::Completed,
                    tool_state: None,
                    payload: NormalizedPayload::Text { text: text.clone() },
                })
                .collect(),
        }],
        retention_limited: false,
    };
    persist_snapshot(&mut connection, &scope, &snapshot, 1).unwrap();
    let owner = AdapterStoreOwner {
        project_id: scope.owner.project_id.clone(),
        chat_id: scope.owner.chat_id.clone(),
        session_id: scope.owner.session_id.clone(),
        runtime_epoch: scope.owner.runtime_epoch,
        turn_id: SageTurnId::parse("turn-one").unwrap(),
        binding_digest: "a".repeat(64),
        external_session_id: ExternalId::new("parent", "thread-1").unwrap(),
    };
    let command = ChildLiveItemCommand {
        child_thread_id: ExternalId::new("child", "child-one").unwrap(),
        child_turn_id: ExternalId::new("turn", "child-turn-1").unwrap(),
        item_id: ExternalId::new("item", "overflow").unwrap(),
        role: None,
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        payload: NormalizedPayload::ThoughtSummary {
            summary: SafeText::new("summary", "overflow omitted").unwrap(),
        },
        now_millis: 2,
    };
    let transaction = connection.transaction().unwrap();
    assert!(
        crate::acp::store::child_transcript::persist_live_item_in(&transaction, &owner, &command)
            .unwrap()
    );
    transaction.commit().unwrap();
    let limited = load_snapshot(&connection, &scope).unwrap().unwrap();
    assert!(limited.retention_limited);
    assert_eq!(limited.turns[0].items.len(), 1024);
    assert!(
        !limited.turns[0]
            .items
            .iter()
            .any(|item| item.id == "overflow")
    );
    let mut foreign = owner.clone();
    foreign.external_session_id = ExternalId::new("parent", "foreign-parent").unwrap();
    let transaction = connection.transaction().unwrap();
    assert!(
        crate::acp::store::child_transcript::persist_live_item_in(&transaction, &foreign, &command)
            .is_err()
    );
    transaction.rollback().unwrap();
    connection.execute("UPDATE session_runtime_epochs SET state='exited',terminal_at=3,exit_kind='processExit' WHERE session_id='session-one' AND runtime_epoch=1", []).unwrap();
    let transaction = connection.transaction().unwrap();
    assert!(
        crate::acp::store::child_transcript::persist_live_item_in(&transaction, &owner, &command)
            .is_err()
    );
    transaction.rollback().unwrap();
    assert_eq!(
        load_earlier_snapshot(&connection, &scope).unwrap(),
        Some(limited)
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn live_command_before_read_message_keeps_witnessed_order() {
    use crate::acp::bindings::codex::{ChildItem, ChildTranscriptSnapshot, ChildTurn};
    use crate::acp::{
        intent::{ToolCategory, ToolExecution},
        lifecycle::{ItemLifecycle, ToolState},
    };

    let directory = std::env::temp_dir().join(format!("sage-cp18e-order-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let mut connection = sqlite_child_owner(&path);
    let scope = scope();
    let retained = ChildTranscriptSnapshot {
        child_thread_id: "child-one".into(),
        parent_thread_id: "thread-1".into(),
        turns: vec![ChildTurn {
            id: "child-turn-1".into(),
            status: "inProgress".into(),
            items: vec![ChildItem {
                id: "command".into(),
                role: None,
                observed_sequence: Some(1),
                lifecycle: ItemLifecycle::Completed,
                tool_state: Some(ToolState::Completed),
                payload: NormalizedPayload::ToolCall {
                    category: ToolCategory::Execute,
                    label: SafeText::new("label", "Review files").unwrap(),
                    input_summary: None,
                    output_summary: Some(SafeText::new("output", "rich live output").unwrap()),
                    locations: Default::default(),
                    command_activity: Default::default(),
                    execution: ToolExecution::default(),
                },
            }],
        }],
        retention_limited: false,
    };
    persist_snapshot(&mut connection, &scope, &retained, 1).unwrap();
    let read = ChildTranscriptSnapshot {
        turns: vec![ChildTurn {
            id: "child-turn-1".into(),
            status: "completed".into(),
            items: vec![
                ChildItem {
                    id: "command".into(),
                    role: None,
                    observed_sequence: None,
                    lifecycle: ItemLifecycle::Completed,
                    tool_state: Some(ToolState::Completed),
                    payload: NormalizedPayload::ToolCall {
                        category: ToolCategory::Execute,
                        label: SafeText::new("label", "Review files").unwrap(),
                        input_summary: None,
                        output_summary: None,
                        locations: Default::default(),
                        command_activity: Default::default(),
                        execution: ToolExecution::default(),
                    },
                },
                ChildItem {
                    id: "message".into(),
                    role: None,
                    observed_sequence: None,
                    lifecycle: ItemLifecycle::Completed,
                    tool_state: None,
                    payload: NormalizedPayload::Text {
                        text: SafeText::new("text", "Result").unwrap(),
                    },
                },
            ],
        }],
        ..retained
    };
    persist_snapshot(&mut connection, &scope, &read, 2).unwrap();
    let stored = load_snapshot(&connection, &scope).unwrap().unwrap();
    assert_eq!(
        stored.turns[0]
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["command", "message"]
    );
    assert!(matches!(&stored.turns[0].items[0].payload,
        NormalizedPayload::ToolCall { output_summary: Some(summary), .. }
            if summary.as_str() == "rich live output"));
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn terminal_parent_turn_does_not_stop_bound_child_retention() {
    use crate::acp::{
        lifecycle::ItemLifecycle,
        store::{AdapterStoreOwner, ChildLiveItemCommand},
    };
    let directory = std::env::temp_dir().join(format!(
        "sage-cp18e-terminal-parent-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let mut connection = sqlite_child_owner(&path);
    connection
        .execute(
            "UPDATE turns SET state='completed',terminal_at=2 WHERE id='turn-one'",
            [],
        )
        .unwrap();
    let scope = scope();
    let owner = AdapterStoreOwner {
        project_id: scope.owner.project_id.clone(),
        chat_id: scope.owner.chat_id.clone(),
        session_id: scope.owner.session_id.clone(),
        runtime_epoch: scope.owner.runtime_epoch,
        turn_id: SageTurnId::parse("other-turn").unwrap(),
        binding_digest: "a".repeat(64),
        external_session_id: ExternalId::new("parent", "thread-1").unwrap(),
    };
    let command = ChildLiveItemCommand {
        child_thread_id: ExternalId::new("child", "child-one").unwrap(),
        child_turn_id: ExternalId::new("turn", "child-turn-1").unwrap(),
        item_id: ExternalId::new("item", "late-command").unwrap(),
        role: None,
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        payload: NormalizedPayload::ThoughtSummary {
            summary: SafeText::new("summary", "Late result").unwrap(),
        },
        now_millis: 3,
    };
    let transaction = connection.transaction().unwrap();
    assert!(
        crate::acp::store::child_transcript::persist_live_item_in(&transaction, &owner, &command)
            .unwrap()
    );
    transaction.commit().unwrap();
    assert_eq!(
        load_snapshot(&connection, &scope).unwrap().unwrap().turns[0].items[0].id,
        "late-command"
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn byte_limit_keeps_admitted_prefix_when_later_item_is_small() {
    use crate::acp::{
        bindings::codex::{ChildItem, ChildTranscriptSnapshot, ChildTurn},
        lifecycle::ItemLifecycle,
        store::{AdapterStoreOwner, ChildLiveItemCommand},
    };
    let directory =
        std::env::temp_dir().join(format!("sage-cp18e-byte-prefix-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let mut connection = sqlite_child_owner(&path);
    let scope = scope();
    let text = SafeText::new("text", "x".repeat(250_000)).unwrap();
    let snapshot = ChildTranscriptSnapshot {
        child_thread_id: "child-one".into(),
        parent_thread_id: "thread-1".into(),
        turns: vec![ChildTurn {
            id: "child-turn-1".into(),
            status: "inProgress".into(),
            items: (0..16)
                .map(|index| ChildItem {
                    id: format!("prefix-{index}"),
                    role: None,
                    observed_sequence: Some(index + 1),
                    lifecycle: ItemLifecycle::Completed,
                    tool_state: None,
                    payload: NormalizedPayload::Text { text: text.clone() },
                })
                .collect(),
        }],
        retention_limited: false,
    };
    persist_snapshot(&mut connection, &scope, &snapshot, 1).unwrap();
    let owner = AdapterStoreOwner {
        project_id: scope.owner.project_id.clone(),
        chat_id: scope.owner.chat_id.clone(),
        session_id: scope.owner.session_id.clone(),
        runtime_epoch: scope.owner.runtime_epoch,
        turn_id: SageTurnId::parse("turn-one").unwrap(),
        binding_digest: "a".repeat(64),
        external_session_id: ExternalId::new("parent", "thread-1").unwrap(),
    };
    for (id, content, expected) in [
        ("over-cap", "y".repeat(200_000), true),
        ("after-gap", "small".into(), false),
    ] {
        let command = ChildLiveItemCommand {
            child_thread_id: ExternalId::new("child", "child-one").unwrap(),
            child_turn_id: ExternalId::new("turn", "child-turn-1").unwrap(),
            item_id: ExternalId::new("item", id).unwrap(),
            role: Some(crate::acp::bindings::codex::ChildTextRole::Assistant),
            lifecycle: ItemLifecycle::Completed,
            tool_state: None,
            payload: NormalizedPayload::Text {
                text: SafeText::new("text", content).unwrap(),
            },
            now_millis: 2,
        };
        let transaction = connection.transaction().unwrap();
        assert_eq!(
            crate::acp::store::child_transcript::persist_live_item_in(
                &transaction,
                &owner,
                &command
            )
            .unwrap(),
            expected
        );
        transaction.commit().unwrap();
    }
    let stored = load_snapshot(&connection, &scope).unwrap().unwrap();
    assert!(stored.retention_limited);
    assert_eq!(stored.turns[0].items.len(), 16);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn child_turn_status_beyond_turn_cap_marks_retention_limited() {
    use crate::acp::{
        bindings::codex::{ChildTranscriptSnapshot, ChildTurn},
        lifecycle::TurnState,
        store::AdapterStoreOwner,
    };
    let directory =
        std::env::temp_dir().join(format!("sage-cp18e-turn-cap-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let mut connection = sqlite_child_owner(&path);
    let scope = scope();
    let snapshot = ChildTranscriptSnapshot {
        child_thread_id: "child-one".into(),
        parent_thread_id: "thread-1".into(),
        turns: (0..128)
            .map(|index| ChildTurn {
                id: format!("turn-{index}"),
                status: "completed".into(),
                items: Vec::new(),
            })
            .collect(),
        retention_limited: false,
    };
    persist_snapshot(&mut connection, &scope, &snapshot, 1).unwrap();
    let owner = AdapterStoreOwner {
        project_id: scope.owner.project_id.clone(),
        chat_id: scope.owner.chat_id.clone(),
        session_id: scope.owner.session_id.clone(),
        runtime_epoch: scope.owner.runtime_epoch,
        turn_id: SageTurnId::parse("turn-one").unwrap(),
        binding_digest: "a".repeat(64),
        external_session_id: ExternalId::new("parent", "thread-1").unwrap(),
    };
    let transaction = connection.transaction().unwrap();
    assert!(
        crate::acp::store::child_transcript::persist_turn_status_in(
            &transaction,
            &owner,
            &ExternalId::new("child", "child-one").unwrap(),
            &ExternalId::new("turn", "turn-129").unwrap(),
            TurnState::Completed,
            2,
        )
        .unwrap()
    );
    transaction.commit().unwrap();
    let stored = load_snapshot(&connection, &scope).unwrap().unwrap();
    assert_eq!(stored.turns.len(), 128);
    assert!(stored.retention_limited);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}
