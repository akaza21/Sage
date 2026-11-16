use super::*;
use sage_acp_protocol::contract::BoundedVec;

fn spawn_row(owner: &AdapterStoreOwner) -> AdapterInboundEvent {
    let key = SafeText::new("agentKey", "child-one").unwrap();
    let mut item = item(owner, "agent:child-one");
    item.payload = NormalizedPayload::Collaboration {
        label: SafeText::new("label", "Subagent").unwrap(),
        message: None,
        receiver_labels: BoundedVec::new(
            "receiverLabels",
            vec![SafeText::new("receiverLabel", "Sleep then report").unwrap()],
        )
        .unwrap(),
        operation: Some(SafeText::new("operation", "spawnAgent").unwrap()),
        agent_key: None,
        agent_keys: BoundedVec::new("agentKeys", vec![key.clone()]).unwrap(),
        agent_states: BoundedVec::new(
            "agentStates",
            vec![(key, SafeText::new("agentState", "running").unwrap())],
        )
        .unwrap(),
        model: None,
        reasoning_effort: None,
    };
    item.payload_digest =
        facts::hex_digest(serde_json::to_string(&item.payload).unwrap().as_bytes());
    item.lifecycle = ItemLifecycle::Started;
    AdapterInboundEvent::ChildThreadItem {
        item,
        child_thread_id: ExternalId::new("native child", "child-one").unwrap(),
    }
}

fn settle(fixture: &mut Fixture, status: TurnState, now_millis: i64) -> AdapterBatchMutation {
    let child = ExternalId::new("native child", "child-one").unwrap();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::ChildTurnStatus {
                child_thread_id: child.clone(),
                child_turn_id: child,
                status,
                now_millis,
            }],
        ),
    )
    .unwrap()
}

fn spawn_states(connection: &rusqlite::Connection) -> Vec<(String, String)> {
    let page = load_chat_page(connection, project(), chat(), None).unwrap();
    page.turns
        .as_slice()
        .iter()
        .flat_map(|turn| turn.items.as_slice())
        .find_map(|item| match &item.content {
            ItemContent::Collaboration { agent_states, .. } => Some(
                agent_states
                    .as_slice()
                    .iter()
                    .map(|(key, state)| (key.as_str().to_owned(), state.as_str().to_owned()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap()
}

fn replace_runtime(connection: &rusqlite::Connection) {
    connection.execute_batch(
        "UPDATE session_runtime_epochs SET state='exited',terminal_at=40,exit_kind='processExit'
           WHERE session_id='session-one' AND runtime_epoch=1;
         INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at)
           VALUES('project-one','chat-one','session-one',2,'running','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',41);",
    ).unwrap();
}

fn running() -> Vec<(String, String)> {
    vec![("child-one".into(), "running".into())]
}

#[test]
fn a_child_settling_after_its_spawn_turn_ended_reprojects_the_spawn_row() {
    let mut fixture = migrated_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![spawn_row(&fixture.owner)]),
    )
    .unwrap();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Completed,
                now_millis: 20,
            }],
        ),
    )
    .unwrap();
    assert_eq!(spawn_states(&fixture.connection), running());
    let before = projection(&fixture.connection);

    let settled = settle(&mut fixture, TurnState::Completed, 30);
    let after = projection(&fixture.connection);
    assert!(after > before, "settling moves the parent projection");
    assert_eq!(settled.projection_revision, after);
    assert_eq!(
        spawn_states(&fixture.connection),
        [("child-one".to_owned(), "completed".to_owned())]
    );

    // A replaced runtime cannot read the transcript, yet the stored end stays.
    replace_runtime(&fixture.connection);
    assert_eq!(
        spawn_states(&fixture.connection),
        [("child-one".to_owned(), "completed".to_owned())]
    );
}

#[test]
fn a_child_settling_inside_its_running_spawn_turn_leaves_the_row_to_the_runtime() {
    let mut fixture = migrated_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![spawn_row(&fixture.owner)]),
    )
    .unwrap();
    let before = projection(&fixture.connection);
    settle(&mut fixture, TurnState::Failed, 30);
    assert_eq!(projection(&fixture.connection), before);
    assert_eq!(spawn_states(&fixture.connection), running());
}

#[test]
fn a_child_without_a_stored_end_stays_live_until_its_runtime_is_gone_then_reads_unknown() {
    let mut fixture = migrated_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                spawn_row(&fixture.owner),
                AdapterInboundEvent::Turn {
                    state: TurnState::Completed,
                    now_millis: 20,
                },
            ],
        ),
    )
    .unwrap();
    let before = projection(&fixture.connection);
    fixture
        .connection
        .execute(
            "INSERT INTO acp_child_thread_transcripts
             (native_child_thread_id,project_id,chat_id,session_id,runtime_epoch,parent_thread_id,
              snapshot_json,snapshot_bytes,updated_at)
             VALUES('child-one','project-one','chat-one','session-one',1,'adapter-session',?1,length(CAST(?1 AS BLOB)),25)",
            [r#"{"childThreadId":"child-one","parentThreadId":"adapter-session","turns":[{"id":"child-one","status":"inProgress","items":[]}]}"#],
        )
        .unwrap();
    assert_eq!(projection(&fixture.connection), before);
    assert_eq!(spawn_states(&fixture.connection), running());

    replace_runtime(&fixture.connection);
    assert_eq!(
        spawn_states(&fixture.connection),
        [("child-one".to_owned(), "unknown".to_owned())]
    );
}
