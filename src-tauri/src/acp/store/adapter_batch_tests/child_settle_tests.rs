use super::*;
use crate::acp::projection::AcpChatPage;
use crate::acp::store::child_transcript::{
    ChildTranscriptScope, ChildTranscriptStoreError, load_snapshot, persist_snapshot,
    settle_lost_children,
};
use sage_acp_protocol::codex_protocol::{
    ChildItem, ChildTranscriptSnapshot, ChildTurn, MAX_ITEMS, MAX_SNAPSHOT_BYTES,
};
use sage_acp_protocol::contract::BoundedVec;
use sage_acp_protocol::scheduler::RuntimeOwner;

fn spawn_row(owner: &AdapterStoreOwner) -> AdapterInboundEvent {
    spawn_row_reporting(owner, "running")
}

fn spawn_row_reporting(owner: &AdapterStoreOwner, state: &str) -> AdapterInboundEvent {
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
            vec![(key, SafeText::new("agentState", state).unwrap())],
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

/// A fixture whose spawn turn has ended with its row reporting the child live.
fn ended_spawn_turn() -> Fixture {
    ended_spawn_turn_reporting("running")
}

fn ended_spawn_turn_reporting(state: &str) -> Fixture {
    let mut fixture = migrated_fixture();
    let ended = AdapterInboundEvent::Turn {
        state: TurnState::Completed,
        now_millis: 20,
    };
    let spawn = spawn_row_reporting(&fixture.owner, state);
    apply(&mut fixture, vec![spawn, ended]);
    fixture
}

fn apply(fixture: &mut Fixture, events: Vec<AdapterInboundEvent>) -> ProjectionRevision {
    apply_adapter_inbound_batch(&mut fixture.connection, batch(&fixture.owner, events))
        .unwrap()
        .projection_revision
}

fn settle(fixture: &mut Fixture, turn: &str, status: TurnState, now_millis: i64) -> bool {
    let before = projection(&fixture.connection);
    let event = AdapterInboundEvent::ChildTurnStatus {
        child_thread_id: ExternalId::new("native child", "child-one").unwrap(),
        child_turn_id: ExternalId::new("turn", turn).unwrap(),
        status,
        now_millis,
    };
    let published = apply(fixture, vec![event]);
    assert_eq!(published, projection(&fixture.connection));
    published > before
}

fn start(fixture: &mut Fixture, turn: &str, now_millis: i64) -> bool {
    delta(fixture, turn, &format!("{turn}-thought"), now_millis)
}

/// Streams one child item of `turn`; true when the parent projection moved.
fn delta(fixture: &mut Fixture, turn: &str, item: &str, now_millis: i64) -> bool {
    let before = projection(&fixture.connection);
    let event = AdapterInboundEvent::ChildLiveItem(ChildLiveItemCommand {
        child_thread_id: ExternalId::new("child", "child-one").unwrap(),
        child_turn_id: ExternalId::new("turn", turn).unwrap(),
        item_id: ExternalId::new("item", item).unwrap(),
        role: None,
        lifecycle: ItemLifecycle::Started,
        tool_state: None,
        payload: NormalizedPayload::ThoughtSummary {
            summary: SafeText::new("summary", "Checking").unwrap(),
        },
        now_millis,
    });
    apply(fixture, vec![event]) > before
}

fn scope(connection: &rusqlite::Connection) -> ChildTranscriptScope {
    ChildTranscriptScope {
        owner: RuntimeOwner {
            project_id: project(),
            chat_id: chat(),
            session_id: SageAgentSessionId::parse("session-one").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        },
        item_id: connection
            .query_row("SELECT item_id FROM acp_child_thread_bindings", [], |row| {
                row.get(0)
            })
            .unwrap(),
        child_thread_id: "child-one".into(),
        parent_thread_id: "adapter-session".into(),
    }
}

fn snapshot(turns: Vec<ChildTurn>) -> ChildTranscriptSnapshot {
    ChildTranscriptSnapshot {
        child_thread_id: "child-one".into(),
        parent_thread_id: "adapter-session".into(),
        turns,
        retention_limited: false,
    }
}

fn turn(id: &str, status: &str, items: Vec<ChildItem>) -> ChildTurn {
    ChildTurn {
        id: id.into(),
        status: status.into(),
        items,
    }
}

fn text_item(id: usize, bytes: usize) -> ChildItem {
    ChildItem {
        id: format!("item-{id}"),
        role: None,
        observed_sequence: Some(id as u64 + 1),
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        payload: NormalizedPayload::Text {
            text: SafeText::new("text", "x".repeat(bytes)).unwrap(),
        },
    }
}

fn page_states(page: &AcpChatPage) -> Vec<String> {
    page.turns
        .as_slice()
        .iter()
        .flat_map(|turn| turn.items.as_slice())
        .find_map(|item| match &item.content {
            ItemContent::Collaboration { agent_states, .. } => Some(
                agent_states
                    .as_slice()
                    .iter()
                    .map(|(_, state)| state.as_str().to_owned())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap()
}

/// The spawn row's end time, and the children it claims no end time for.
fn spawn_times(connection: &rusqlite::Connection) -> (i64, Vec<String>) {
    let page = load_chat_page(connection, project(), chat(), None).unwrap();
    page.turns
        .as_slice()
        .iter()
        .flat_map(|turn| turn.items.as_slice())
        .find_map(|item| match &item.content {
            ItemContent::Collaboration { untimed_agents, .. } => Some((
                item.updated_at,
                untimed_agents
                    .iter()
                    .map(|key| key.as_str().to_owned())
                    .collect(),
            )),
            _ => None,
        })
        .unwrap()
}

fn spawn_state(connection: &rusqlite::Connection) -> String {
    page_states(&load_chat_page(connection, project(), chat(), None).unwrap()).join(",")
}

#[test]
fn a_child_settling_after_its_spawn_turn_ended_reprojects_the_spawn_row_once() {
    let mut fixture = ended_spawn_turn();
    assert_eq!(spawn_state(&fixture.connection), "running");

    assert!(settle(&mut fixture, "child-turn", TurnState::Completed, 30));
    assert_eq!(spawn_state(&fixture.connection), "completed");
    assert!(!settle(
        &mut fixture,
        "child-turn",
        TurnState::Completed,
        31
    ));
    assert_eq!(
        spawn_times(&fixture.connection),
        (30, Vec::new()),
        "the end is dated when it was observed"
    );

    // The settled state is read back without the child's transcript.
    fixture
        .connection
        .execute("DELETE FROM acp_child_thread_transcripts", [])
        .unwrap();
    assert_eq!(spawn_state(&fixture.connection), "completed");
}

#[test]
fn a_child_settling_inside_its_running_spawn_turn_leaves_the_row_to_the_runtime() {
    let mut fixture = migrated_fixture();
    let spawn = spawn_row(&fixture.owner);
    apply(&mut fixture, vec![spawn]);
    assert!(!settle(&mut fixture, "child-turn", TurnState::Failed, 30));
    assert_eq!(spawn_state(&fixture.connection), "running");
}

#[test]
fn a_child_turn_after_its_spawn_turn_ended_moves_the_row_both_ways() {
    let mut fixture = ended_spawn_turn();
    assert!(
        !start(&mut fixture, "first", 21),
        "the row already shows it running"
    );
    assert!(settle(&mut fixture, "first", TurnState::Completed, 30));
    assert_eq!(spawn_state(&fixture.connection), "completed");

    assert!(start(&mut fixture, "second", 40));
    assert_eq!(spawn_state(&fixture.connection), "running");
    assert!(!start(&mut fixture, "second", 41));
    assert!(settle(&mut fixture, "second", TurnState::Interrupted, 50));
    assert_eq!(spawn_state(&fixture.connection), "interrupted");
    assert!(!settle(&mut fixture, "first", TurnState::Completed, 51));
    assert_eq!(spawn_state(&fixture.connection), "interrupted");
}

#[test]
fn a_history_read_behind_a_live_turn_keeps_that_turn_current() {
    let mut fixture = ended_spawn_turn();
    let scope = scope(&fixture.connection);
    assert!(!start(&mut fixture, "second", 21));
    let read = snapshot(vec![
        turn("first", "completed", Vec::new()),
        turn("second", "inProgress", Vec::new()),
    ]);
    assert_eq!(
        persist_snapshot(&mut fixture.connection, &scope, &read, 25).unwrap(),
        None
    );
    assert_eq!(spawn_state(&fixture.connection), "running");
    let stored = load_snapshot(&fixture.connection, &scope).unwrap().unwrap();
    let order: Vec<&str> = stored.turns.iter().map(|turn| turn.id.as_str()).collect();
    assert_eq!(order, ["first", "second"]);

    assert!(settle(&mut fixture, "second", TurnState::Failed, 30));
    assert_eq!(spawn_state(&fixture.connection), "errored");
}

#[test]
fn a_native_read_that_settles_the_child_publishes_the_spawn_row() {
    let mut fixture = ended_spawn_turn();
    let scope = scope(&fixture.connection);
    let running = snapshot(vec![turn("child-turn", "inProgress", Vec::new())]);
    assert_eq!(
        persist_snapshot(&mut fixture.connection, &scope, &running, 25).unwrap(),
        None
    );
    let before = projection(&fixture.connection);
    let ended = snapshot(vec![turn("child-turn", "failed", Vec::new())]);
    let published = persist_snapshot(&mut fixture.connection, &scope, &ended, 30).unwrap();
    assert!(published.is_some_and(|revision| revision > before));
    assert_eq!(published, Some(projection(&fixture.connection)));
    assert_eq!(spawn_state(&fixture.connection), "errored");
    assert_eq!(
        spawn_times(&fixture.connection).1,
        ["child-one"],
        "a read learns no end time"
    );
}

#[test]
fn a_child_end_past_the_turn_cap_still_settles_the_spawn_row() {
    let mut fixture = ended_spawn_turn();
    let scope = scope(&fixture.connection);
    persist_snapshot(&mut fixture.connection, &scope, &capped_transcript(), 25).unwrap();
    assert_eq!(spawn_state(&fixture.connection), "running");

    assert!(settle(&mut fixture, "turn-128", TurnState::Failed, 30));
    assert_eq!(spawn_state(&fixture.connection), "errored");
    assert!(start(&mut fixture, "turn-129", 40));
    assert_eq!(spawn_state(&fixture.connection), "running");
    assert!(settle(&mut fixture, "turn-129", TurnState::Completed, 50));
    assert_eq!(spawn_state(&fixture.connection), "completed");
    for (turn, now) in [("turn-129", 60), ("turn-128", 61)] {
        assert!(!start(&mut fixture, turn, now), "{turn} never runs again");
        assert_eq!(spawn_state(&fixture.connection), "completed");
    }
}

fn capped_transcript() -> ChildTranscriptSnapshot {
    let mut turns: Vec<ChildTurn> = (0..127)
        .map(|index| turn(&format!("turn-{index}"), "completed", Vec::new()))
        .collect();
    turns.push(turn("turn-127", "inProgress", Vec::new()));
    snapshot(turns)
}

#[test]
fn a_delayed_read_never_reopens_a_turn_the_record_moved_past() {
    let mut fixture = ended_spawn_turn();
    let scope = scope(&fixture.connection);
    let read = capped_transcript();
    persist_snapshot(&mut fixture.connection, &scope, &read, 25).unwrap();
    assert!(settle(&mut fixture, "turn-128", TurnState::Failed, 30));
    assert_eq!(spawn_state(&fixture.connection), "errored");

    // The same read, issued while turn-127 still ran, lands after turn-128
    // ended; the transcript is full and never held turn-128.
    assert_eq!(
        persist_snapshot(&mut fixture.connection, &scope, &read, 35).unwrap(),
        None
    );
    assert_eq!(spawn_state(&fixture.connection), "errored");
    let recorded: (String, i64) = fixture
        .connection
        .query_row(
            "SELECT turn_id,turn_ordinal FROM acp_child_thread_states",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(recorded, ("turn-128".into(), 128));
}

#[test]
fn a_read_naming_a_turn_twice_is_rejected() {
    let mut fixture = ended_spawn_turn();
    let scope = scope(&fixture.connection);
    let twice = snapshot(vec![
        turn("first", "completed", Vec::new()),
        turn("first", "failed", Vec::new()),
    ]);
    assert!(matches!(
        persist_snapshot(&mut fixture.connection, &scope, &twice, 25),
        Err(ChildTranscriptStoreError::Corrupt)
    ));
    assert!(
        load_snapshot(&fixture.connection, &scope)
            .unwrap()
            .is_none()
    );

    let once = snapshot(vec![turn("first", "inProgress", Vec::new())]);
    persist_snapshot(&mut fixture.connection, &scope, &once, 26).unwrap();
    assert!(matches!(
        persist_snapshot(&mut fixture.connection, &scope, &twice, 27),
        Err(ChildTranscriptStoreError::Corrupt)
    ));
    let stored = load_snapshot(&fixture.connection, &scope).unwrap().unwrap();
    assert_eq!(stored.turns, once.turns);
    assert_eq!(spawn_state(&fixture.connection), "running");
}

#[test]
fn a_late_delta_of_an_ended_turn_never_changes_the_spawn_row() {
    let mut fixture = ended_spawn_turn();
    assert!(!start(&mut fixture, "first", 21));
    assert!(settle(&mut fixture, "first", TurnState::Completed, 22));
    assert!(start(&mut fixture, "second", 23));
    assert!(settle(&mut fixture, "second", TurnState::Failed, 24));
    for (turn, now) in [("first", 25), ("second", 26)] {
        assert!(!delta(&mut fixture, turn, &format!("{turn}-late"), now));
        assert_eq!(spawn_state(&fixture.connection), "errored");
    }
}

#[test]
fn a_spawn_row_closed_after_a_later_child_turn_began_keeps_its_own_state() {
    let mut fixture = migrated_fixture();
    let spawn = spawn_row(&fixture.owner);
    apply(&mut fixture, vec![spawn]);
    assert!(!start(&mut fixture, "first", 21));
    assert!(!settle(&mut fixture, "first", TurnState::Completed, 22));
    // The parent sends the child more input within the same turn, and the
    // end of that second child turn is never observed.
    assert!(!start(&mut fixture, "second", 23));
    let closed = spawn_row_reporting(&fixture.owner, "completed");
    let ended = AdapterInboundEvent::Turn {
        state: TurnState::Completed,
        now_millis: 30,
    };
    apply(&mut fixture, vec![closed, ended]);
    assert_eq!(spawn_state(&fixture.connection), "completed");
}

/// The spawn row, written as `completed`, after an upgrade that found the
/// child's transcript already holding `turns`.
fn upgraded_spawn_state(turns: Vec<ChildTurn>) -> String {
    let mut fixture = ended_spawn_turn_reporting("completed");
    let scope = scope(&fixture.connection);
    persist_snapshot(&mut fixture.connection, &scope, &snapshot(turns), 25).unwrap();
    fixture
        .connection
        .execute_batch(&format!(
            "DROP TABLE acp_child_thread_states; {}",
            include_str!("../../../../migrations/031_acp_child_thread_states.sql")
        ))
        .unwrap();
    spawn_state(&fixture.connection)
}

#[test]
fn an_upgrade_shows_a_child_turn_that_ended_after_the_one_its_row_reports() {
    assert_eq!(
        upgraded_spawn_state(vec![
            turn("first", "completed", Vec::new()),
            turn("second", "failed", Vec::new()),
        ]),
        "errored"
    );
    assert_eq!(
        upgraded_spawn_state(vec![turn("first", "failed", Vec::new())]),
        "completed",
        "the row speaks for the turn the child was spawned with"
    );
}

#[test]
fn a_child_turn_after_its_row_was_rewritten_settled_still_shows_running() {
    let mut fixture = ended_spawn_turn_reporting("completed");
    assert_eq!(spawn_state(&fixture.connection), "completed");
    // The clock stepped back below the row's own time: order, not time, counts.
    assert!(start(&mut fixture, "second", 5));
    assert_eq!(spawn_state(&fixture.connection), "running");
    assert!(settle(&mut fixture, "second", TurnState::Failed, 6));
    assert_eq!(spawn_state(&fixture.connection), "errored");
}

#[test]
fn a_child_turn_past_the_item_cap_moves_the_spawn_row_both_ways() {
    let mut fixture = ended_spawn_turn();
    let scope = scope(&fixture.connection);
    let items = (0..MAX_ITEMS).map(|id| text_item(id, 1)).collect();
    let full = snapshot(vec![turn("first", "completed", items)]);
    persist_snapshot(&mut fixture.connection, &scope, &full, 25).unwrap();
    assert_eq!(spawn_state(&fixture.connection), "completed");

    assert!(start(&mut fixture, "second", 30));
    assert_eq!(spawn_state(&fixture.connection), "running");
    assert!(settle(&mut fixture, "second", TurnState::Completed, 40));
    assert_eq!(spawn_state(&fixture.connection), "completed");
    assert!(!start(&mut fixture, "first", 50), "first never runs again");
    assert_eq!(spawn_state(&fixture.connection), "completed");
}

#[test]
fn a_child_turn_past_the_byte_cap_moves_the_spawn_row_both_ways() {
    let mut fixture = ended_spawn_turn();
    let scope = scope(&fixture.connection);
    let mut items: Vec<ChildItem> = (0..16).map(|id| text_item(id, 250_000)).collect();
    items.push(text_item(16, 0));
    let mut full = snapshot(vec![turn("first", "completed", items)]);
    let spare = MAX_SNAPSHOT_BYTES - 64 - serde_json::to_vec(&full).unwrap().len();
    full.turns[0].items[16] = text_item(16, spare - 8);
    persist_snapshot(&mut fixture.connection, &scope, &full, 25).unwrap();
    assert_eq!(spawn_state(&fixture.connection), "completed");

    assert!(start(&mut fixture, "second", 30));
    assert_eq!(spawn_state(&fixture.connection), "running");
    assert!(settle(&mut fixture, "second", TurnState::Failed, 40));
    assert_eq!(spawn_state(&fixture.connection), "errored");
}

#[test]
fn a_live_child_of_an_ended_turn_reads_unknown_once_nothing_can_run_it() {
    let fixture = ended_spawn_turn();
    let mut page = load_chat_page(&fixture.connection, project(), chat(), None).unwrap();
    let mut held = page.clone();
    settle_lost_children(&mut held, |owner| owner.runtime_epoch.get() == 1).unwrap();
    assert_eq!(page_states(&held), ["running"]);

    // The app restarted, or the runtime exited, without recording an end.
    settle_lost_children(&mut page, |_| false).unwrap();
    assert_eq!(page_states(&page), ["unknown"]);

    // A recovery that ends the epoch durably settles it while still held.
    fixture
        .connection
        .execute(
            "UPDATE session_runtime_epochs SET state='exited',terminal_at=40,exit_kind='processExit'
               WHERE session_id='session-one' AND runtime_epoch=1",
            [],
        )
        .unwrap();
    let mut ended = load_chat_page(&fixture.connection, project(), chat(), None).unwrap();
    settle_lost_children(&mut ended, |_| true).unwrap();
    assert_eq!(page_states(&ended), ["unknown"]);
}

#[test]
fn a_live_child_of_an_earlier_session_reads_unknown() {
    let fixture = ended_spawn_turn();
    fixture.connection.execute_batch(&format!(
        "DELETE FROM chat_session_heads WHERE chat_id='chat-one';
         UPDATE session_runtime_epochs SET state='exited',terminal_at=40,exit_kind='processExit'
           WHERE session_id='session-one';
         UPDATE agent_sessions SET state='closing',revision=revision+1,updated_at=40 WHERE id='session-one';
         UPDATE agent_sessions SET state='closed',residency='cold',revision=revision+1,updated_at=40,terminal_at=40
           WHERE id='session-one';
         INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-two','project-one','chat-one',1,'binding','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','adapter-session-two','{}','active','hot',0,0,0,60,60);
         INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at)
           VALUES('project-one','chat-one','session-two',0,60);",
        capabilities(),
    )).unwrap();
    let mut page = load_chat_page(&fixture.connection, project(), chat(), None).unwrap();
    assert_eq!(
        page.current_session_id.as_ref().map(|id| id.as_str()),
        Some("session-two")
    );
    settle_lost_children(&mut page, |_| true).unwrap();
    assert_eq!(page_states(&page), ["unknown"]);
}
