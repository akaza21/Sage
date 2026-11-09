use super::*;

fn child_item(owner: &AdapterStoreOwner, external: &str, child: &str) -> AdapterInboundEvent {
    let mut item = item(owner, external);
    item.payload = NormalizedPayload::Collaboration {
        label: SafeText::new("label", "Worker").unwrap(),
        message: None,
        receiver_labels: Default::default(),
        operation: Some(SafeText::new("operation", "started").unwrap()),
        agent_key: None,
        agent_keys: Default::default(),
        agent_states: Default::default(),
        model: None,
        reasoning_effort: None,
    };
    item.payload_digest =
        facts::hex_digest(serde_json::to_string(&item.payload).unwrap().as_bytes());
    item.lifecycle = ItemLifecycle::Started;
    AdapterInboundEvent::ChildThreadItem {
        item,
        child_thread_id: ExternalId::new("native child", child).unwrap(),
    }
}

fn binding(
    connection: &rusqlite::Connection,
    external: &str,
) -> (String, String, String, i64, String) {
    connection
        .query_row(
            "SELECT b.session_id,b.turn_id,b.item_id,b.runtime_epoch,b.native_child_thread_id
         FROM acp_child_thread_bindings b JOIN items i ON i.id=b.item_id
         WHERE i.external_item_id=?1",
            [external],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap()
}

#[test]
fn native_child_identity_commits_with_exact_parent_item_and_replays() {
    let mut fixture = migrated_fixture();
    let fact = child_item(&fixture.owner, "activity-one", "child-one");
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![fact.clone()]),
    )
    .unwrap();
    let stored = binding(&fixture.connection, "activity-one");
    assert_eq!(stored.0, fixture.owner.session_id.as_str());
    assert_eq!(stored.1, fixture.owner.turn_id.as_str());
    assert_eq!(stored.3, 1);
    assert_eq!(stored.4, "child-one");
    apply_adapter_inbound_batch(&mut fixture.connection, batch(&fixture.owner, vec![fact]))
        .unwrap();
    assert_eq!(binding(&fixture.connection, "activity-one"), stored);
    let owners: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM acp_child_thread_owners", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(owners, 1);
}

#[test]
fn native_child_identity_conflict_rolls_back_the_entire_batch() {
    let mut fixture = migrated_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![child_item(&fixture.owner, "activity-one", "child-one")],
        ),
    )
    .unwrap();
    let before = projection(&fixture.connection);
    let result = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                AdapterInboundEvent::Item(item(&fixture.owner, "unrelated")),
                child_item(&fixture.owner, "activity-one", "child-two"),
            ],
        ),
    );
    assert!(matches!(
        result,
        Err(StoreError::Conflict {
            resource: "childThreadItem"
        })
    ));
    assert_eq!(binding(&fixture.connection, "activity-one").4, "child-one");
    let extra: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM items WHERE external_item_id='unrelated'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!((extra, projection(&fixture.connection)), (0, before));
}

#[test]
fn native_child_identity_survives_terminal_item_in_same_batch() {
    let mut fixture = migrated_fixture();
    let child = child_item(&fixture.owner, "activity-one", "child-one");
    let mut terminal = match &child {
        AdapterInboundEvent::ChildThreadItem { item, .. } => item.clone(),
        _ => unreachable!(),
    };
    terminal.lifecycle = ItemLifecycle::Completed;
    terminal.now_millis = 7;
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![child, AdapterInboundEvent::Item(terminal)],
        ),
    )
    .unwrap();
    assert_eq!(binding(&fixture.connection, "activity-one").4, "child-one");
    let lifecycle: String = fixture
        .connection
        .query_row(
            "SELECT lifecycle FROM items WHERE external_item_id='activity-one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(lifecycle, "completed");
}

#[test]
fn native_child_identity_can_link_multiple_items_of_one_parent_only() {
    let mut fixture = migrated_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![child_item(&fixture.owner, "activity-one", "child-one")],
        ),
    )
    .unwrap();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![child_item(&fixture.owner, "activity-two", "child-one")],
        ),
    )
    .unwrap();
    assert_eq!(binding(&fixture.connection, "activity-two").4, "child-one");
    let owners: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM acp_child_thread_owners", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(owners, 1);

    fixture.connection.execute_batch(
        "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
         VALUES('chat-two','project-one',1,'Other',0,1,1);
         INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at)
         SELECT 'session-two',project_id,'chat-two',0,binding_id,binding_digest,'other-parent',capabilities_json,state,residency,legacy,0,0,created_at,updated_at
         FROM agent_sessions WHERE id='session-one';
         INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at)
         VALUES('project-one','chat-two','session-two',1,'running','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',1);
         INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at)
         VALUES('turn-two','project-one','chat-two','session-two',1,0,'intent-two','running','correlated',0,1,1);
         INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,item_bytes,fact_count,fact_bytes,payload_bytes,revision)
         VALUES('project-one','chat-two','session-two',1,0,0,0,0,0,0);
         UPDATE acp_project_usage SET chat_count=2,session_count=2,turn_count=2 WHERE project_id='project-one';
         UPDATE acp_store_usage SET chat_count=2,session_count=2,turn_count=2 WHERE singleton=1;",
    ).unwrap();
    let mut other = fixture.owner.clone();
    other.chat_id = SageChatId::parse("chat-two").unwrap();
    other.session_id = SageAgentSessionId::parse("session-two").unwrap();
    other.turn_id = SageTurnId::parse("turn-two").unwrap();
    other.external_session_id = ExternalId::new("external", "other-parent").unwrap();
    let result = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &other,
            vec![child_item(&other, "activity-other", "child-one")],
        ),
    );
    assert!(matches!(
        result,
        Err(StoreError::Conflict {
            resource: "childThreadOwner"
        })
    ));
    let foreign_items: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM items WHERE session_id='session-two'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(foreign_items, 0);
}

#[test]
fn native_child_identity_rejects_noncollaboration_and_parent_self_binding() {
    let mut fixture = migrated_fixture();
    let mut noncollaboration = child_item(&fixture.owner, "bad-kind", "child-one");
    if let AdapterInboundEvent::ChildThreadItem { item, .. } = &mut noncollaboration {
        item.payload = NormalizedPayload::Text {
            text: SafeText::new("text", "not collaboration").unwrap(),
        };
        item.payload_digest =
            facts::hex_digest(serde_json::to_string(&item.payload).unwrap().as_bytes());
    }
    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(&fixture.owner, vec![noncollaboration])
        ),
        Err(StoreError::Conflict {
            resource: "childThreadItem"
        })
    ));
    let self_id = fixture.owner.external_session_id.as_str();
    let result = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![child_item(&fixture.owner, "self", self_id)],
        ),
    );
    assert!(matches!(
        result,
        Err(StoreError::Conflict {
            resource: "childThreadItem"
        })
    ));
    let count: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM acp_child_thread_bindings",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn native_child_identity_reopens_without_enabling_transcript_capability() {
    let mut fixture = migrated_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![child_item(&fixture.owner, "activity-one", "child-one")],
        ),
    )
    .unwrap();
    let directory =
        std::env::temp_dir().join(format!("sage-child-binding-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    {
        let mut copy = rusqlite::Connection::open(&path).unwrap();
        rusqlite::backup::Backup::new(&fixture.connection, &mut copy)
            .unwrap()
            .run_to_completion(256, std::time::Duration::ZERO, None)
            .unwrap();
    }
    drop(fixture);
    let reopened = crate::acp::migration::open_or_migrate(&path).unwrap();
    assert_eq!(binding(&reopened, "activity-one").4, "child-one");
    let capabilities: String = reopened
        .query_row(
            "SELECT capabilities_json FROM agent_sessions WHERE id='session-one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let capabilities: serde_json::Value = serde_json::from_str(&capabilities).unwrap();
    assert_eq!(capabilities["childTranscriptRead"], false);
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn child_transcript_write_rechecks_binding_and_current_epoch() {
    use crate::acp::{
        bindings::codex::child_read_for_test,
        scheduler::RuntimeOwner,
        store::child_transcript::{
            ChildTranscriptScope, load_earlier_snapshot, load_snapshot, persist_snapshot,
        },
    };

    let mut fixture = migrated_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![child_item(&fixture.owner, "activity-one", "child-one")],
        ),
    )
    .unwrap();
    let item_id = binding(&fixture.connection, "activity-one").2;
    let scope = ChildTranscriptScope {
        owner: RuntimeOwner {
            project_id: fixture.owner.project_id.clone(),
            chat_id: fixture.owner.chat_id.clone(),
            session_id: fixture.owner.session_id.clone(),
            runtime_epoch: fixture.owner.runtime_epoch,
        },
        item_id,
        child_thread_id: "child-one".into(),
        parent_thread_id: "adapter-session".into(),
    };
    let wire = br#"{"id":17,"result":{"thread":{"id":"child-one","parentThreadId":"adapter-session","turns":[{"id":"native-turn","status":"completed","itemsView":"full","items":[{"id":"answer","type":"agentMessage","text":"Result","phase":null,"memoryCitation":null}]}]}}}"#;
    let mapper = crate::acp::resource_mapper::NativeResourceMapper::new([]).unwrap();
    let snapshot = child_read_for_test(wire, 17, "child-one", "adapter-session", &mapper).unwrap();
    assert!(persist_snapshot(&mut fixture.connection, &scope, &snapshot, 10).is_ok());
    assert_eq!(
        load_snapshot(&fixture.connection, &scope).unwrap(),
        Some(snapshot.clone())
    );

    let mut foreign = scope.clone();
    foreign.child_thread_id = "child-two".into();
    assert!(persist_snapshot(&mut fixture.connection, &foreign, &snapshot, 11).is_err());
    foreign = scope.clone();
    foreign.parent_thread_id = "other-parent".into();
    assert!(persist_snapshot(&mut fixture.connection, &foreign, &snapshot, 11).is_err());
    foreign = scope.clone();
    foreign.item_id = "item-one".into();
    assert!(persist_snapshot(&mut fixture.connection, &foreign, &snapshot, 11).is_err());
    foreign = scope.clone();
    foreign.owner.session_id = SageAgentSessionId::parse("session-other").unwrap();
    assert!(persist_snapshot(&mut fixture.connection, &foreign, &snapshot, 11).is_err());
    foreign = scope.clone();
    foreign.owner.runtime_epoch = RuntimeEpoch::new(2).unwrap();
    assert!(persist_snapshot(&mut fixture.connection, &foreign, &snapshot, 11).is_err());
    fixture
        .connection
        .execute(
            "DELETE FROM chat_session_heads WHERE chat_id='chat-one'",
            [],
        )
        .unwrap();
    assert!(load_snapshot(&fixture.connection, &scope).is_err());
    fixture.connection.execute(
        "INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1)", [],
    ).unwrap();
    fixture.connection.execute(
        "UPDATE session_runtime_epochs SET state='terminating' WHERE session_id='session-one' AND runtime_epoch=1", [],
    ).unwrap();
    assert!(persist_snapshot(&mut fixture.connection, &scope, &snapshot, 12).is_err());
    assert!(load_snapshot(&fixture.connection, &scope).is_err());
    fixture.connection.execute(
        "UPDATE session_runtime_epochs SET state='exited',terminal_at=12,exit_kind='processExit' WHERE session_id='session-one' AND runtime_epoch=1", [],
    ).unwrap();
    assert_eq!(
        load_earlier_snapshot(&fixture.connection, &scope).unwrap(),
        Some(snapshot.clone())
    );
    let mut foreign = scope.clone();
    foreign.parent_thread_id = "other-parent".into();
    assert!(
        load_earlier_snapshot(&fixture.connection, &foreign)
            .unwrap()
            .is_none()
    );
    fixture
        .connection
        .execute(
            "DELETE FROM chat_session_heads WHERE chat_id='chat-one'",
            [],
        )
        .unwrap();
    assert!(
        load_earlier_snapshot(&fixture.connection, &scope)
            .unwrap()
            .is_none()
    );
    fixture.connection.execute(
        "INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1)", [],
    ).unwrap();
    fixture.connection.execute(
        "INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-one','chat-one','session-one',2,'running',?1,13)",
        ["d".repeat(64)],
    ).unwrap();
    assert!(
        load_earlier_snapshot(&fixture.connection, &scope)
            .unwrap()
            .is_none()
    );
}
