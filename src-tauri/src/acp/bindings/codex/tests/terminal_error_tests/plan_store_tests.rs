use super::*;
use crate::acp::{
    projection::{ItemContent, PlanEntryState},
    store::{
        AdapterInboundBatchCommand, AdapterInboundEvent, AdapterStoreOwner, ItemIngressCommand,
        apply_adapter_inbound_batch, hex_digest, load_chat_page,
    },
};
use sage_acp_protocol::{
    contract::{SageChatId, SageProjectId, SageTurnId},
    lifecycle::ItemLifecycle,
};

#[test]
fn stopped_turns_persist_cancelled_plans_with_their_entries_intact() {
    for (user_stop, status, expected_turn) in [
        (true, "interrupted", TurnState::Cancelled),
        (false, "interrupted", TurnState::Interrupted),
        (false, "failed", TurnState::Failed),
    ] {
        let mut adapter = running_adapter();
        let (mut connection, owner) = store(&adapter);
        let plan = json!({"method":"turn/plan/updated","params":{
            "threadId":"thread-1","turnId":"turn-1","explanation":null,
            "plan":[
                {"step":"Inspect","status":"completed"},
                {"step":"Implement","status":"inProgress"},
                {"step":"Verify","status":"pending"}
            ]
        }});
        let IngressDisposition::Events(events) = adapter
            .ingest(&serde_json::to_vec(&plan).unwrap(), &Mapper)
            .unwrap()
        else {
            panic!("expected the initial plan");
        };
        apply(&mut connection, &owner, events, 7);
        assert_plan(
            &connection,
            &owner,
            TurnState::Running,
            ItemLifecycle::Streaming,
        );

        if user_stop {
            adapter.cancel(3).unwrap();
        }
        let terminal = json!({"method":"turn/completed","params":{
            "threadId":"thread-1","turn":{"id":"turn-1","status":status}
        }});
        let IngressDisposition::Events(events) = adapter
            .ingest(&serde_json::to_vec(&terminal).unwrap(), &Mapper)
            .unwrap()
        else {
            panic!("expected the terminal turn");
        };
        assert!(matches!(
            events.as_slice(), [IncomingEvent::Turn { state, .. }] if *state == expected_turn
        ));
        apply(&mut connection, &owner, events, 9);
        assert_plan(&connection, &owner, expected_turn, ItemLifecycle::Cancelled);
    }
}

fn assert_plan(
    connection: &rusqlite::Connection,
    owner: &AdapterStoreOwner,
    turn_state: TurnState,
    lifecycle: ItemLifecycle,
) {
    let page = load_chat_page(
        connection,
        owner.project_id.clone(),
        owner.chat_id.clone(),
        None,
    )
    .unwrap();
    let [turn] = page.turns.as_slice() else {
        panic!("expected the persisted turn");
    };
    assert_eq!(turn.state, turn_state);
    let [plan] = turn.items.as_slice() else {
        panic!("expected the persisted plan");
    };
    assert_eq!(plan.lifecycle, lifecycle);
    assert_eq!(plan.terminal_at, lifecycle.is_terminal().then_some(9));
    let ItemContent::Plan { entries } = &plan.content else {
        panic!("expected structured plan entries");
    };
    assert_eq!(
        entries
            .as_slice()
            .iter()
            .map(|entry| (entry.label.as_str(), entry.state))
            .collect::<Vec<_>>(),
        [
            ("Inspect", PlanEntryState::Completed),
            ("Implement", PlanEntryState::InProgress),
            ("Verify", PlanEntryState::Pending),
        ]
    );
}

fn apply(
    connection: &mut rusqlite::Connection,
    owner: &AdapterStoreOwner,
    events: Vec<IncomingEvent>,
    now_millis: i64,
) {
    let events = events
        .into_iter()
        .map(|event| match event {
            IncomingEvent::Item {
                id,
                lifecycle,
                tool_state,
                payload,
                ..
            } => {
                let payload_digest = hex_digest(&serde_json::to_vec(&payload).unwrap());
                AdapterInboundEvent::Item(ItemIngressCommand {
                    project_id: owner.project_id.clone(),
                    chat_id: owner.chat_id.clone(),
                    session_id: owner.session_id.clone(),
                    turn_id: owner.turn_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    external_item_id: id.external_id,
                    activity_id: None,
                    payload,
                    payload_digest,
                    lifecycle,
                    tool_state,
                    now_millis,
                })
            }
            IncomingEvent::Turn { state, .. } => AdapterInboundEvent::Turn { state, now_millis },
            other => panic!("unexpected plan or terminal event: {other:?}"),
        })
        .collect();
    let batch = AdapterInboundBatchCommand::new(owner.clone(), 1024, 4096, events).unwrap();
    apply_adapter_inbound_batch(connection, batch).unwrap();
}

fn store(adapter: &CodexAdapter) -> (rusqlite::Connection, AdapterStoreOwner) {
    let (connection, _) = crate::acp::migration::open_or_migrate(std::path::Path::new(":memory:"))
        .unwrap()
        .into_parts();
    connection.execute_batch(&format!(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Plan',1,1,1);
         INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-one','project-one','C:/repo','c:/repo',0);
         INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Plan',0,1,1);
         INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-one','project-one','chat-one',0,'binding','{digest}','thread-1','{capabilities}','active','hot',0,0,0,1,1);
         INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1);
         INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-one','chat-one','session-one',1,'running','{artifact}',1);
         INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-one','project-one','chat-one','session-one',1,0,'intent-one','running','correlated',0,1,1);
         INSERT INTO acp_project_usage VALUES('project-one',1,1,1,0,0,0,0);
         INSERT INTO acp_session_usage VALUES('project-one','chat-one','session-one',1,0,0,0,0,0,0);
         UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=1 WHERE singleton=1;",
        digest = "a".repeat(64),
        artifact = "d".repeat(64),
        capabilities = serde_json::to_string(&adapter.effective_capabilities().unwrap()).unwrap(),
    )).unwrap();
    let owner = AdapterStoreOwner {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        turn_id: SageTurnId::parse("turn-one").unwrap(),
        binding_digest: "a".repeat(64),
        external_session_id: ExternalId::new("external", "thread-1").unwrap(),
    };
    (connection, owner)
}
