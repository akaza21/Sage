use super::*;
use crate::acp::intent::session_event::{
    SessionActivityState::{self, Cancelled, Completed, Failed, Running},
    SessionNoticeLevel, SessionScopedEvent, SessionSurface,
    fixtures::{activity, notice},
};
use serde_json::{Value, json};

fn session(event: SessionScopedEvent) -> AdapterInboundEvent {
    AdapterInboundEvent::Session(SessionEventCommand {
        event,
        now_millis: 8,
    })
}

fn turn(state: TurnState) -> AdapterInboundEvent {
    AdapterInboundEvent::Turn {
        state,
        now_millis: 7,
    }
}

fn hook(id: &str, state: SessionActivityState, progress: &str) -> AdapterInboundEvent {
    let mut row = activity(id, state, progress);
    row.label = RequiredText::new("label", id).unwrap();
    session(SessionScopedEvent::Activity(row))
}

fn late_hook(id: &str, state: SessionActivityState) -> AdapterInboundEvent {
    let mut row = activity(id, state, "settled late");
    row.label = RequiredText::new("label", id).unwrap();
    row.in_previous_turn = true;
    session(SessionScopedEvent::Activity(row))
}

fn apply(fixture: &mut Fixture, owner: &AdapterStoreOwner, events: Vec<AdapterInboundEvent>) {
    apply_adapter_inbound_batch(&mut fixture.connection, batch(owner, events)).unwrap();
}

fn rows(fixture: &Fixture) -> Vec<Vec<(String, String, Option<String>)>> {
    let page = load_chat_page(
        &fixture.connection,
        fixture.owner.project_id.clone(),
        fixture.owner.chat_id.clone(),
        None,
    )
    .unwrap();
    let mut turns = page.turns.as_slice().iter().collect::<Vec<_>>();
    turns.sort_by_key(|turn| turn.ordinal);
    turns
        .into_iter()
        .map(|turn| {
            let value = serde_json::to_value(turn).unwrap();
            let Some(Value::Array(rows)) = value.get("sessionActivities") else {
                return Vec::new();
            };
            rows.iter()
                .map(|row| {
                    assert_eq!(row.get("id"), None, "the harness identity stays native");
                    (
                        row["label"].as_str().unwrap().to_owned(),
                        row["state"].as_str().unwrap().to_owned(),
                        row.get("progress").map(|p| p.as_str().unwrap().to_owned()),
                    )
                })
                .collect()
        })
        .collect()
}

fn row(label: &str, state: &str, progress: &str) -> (String, String, Option<String>) {
    (label.into(), state.into(), Some(progress.into()))
}

fn next_turn(fixture: &mut Fixture, owner: &AdapterStoreOwner, intent: &str) -> AdapterStoreOwner {
    apply(fixture, owner, vec![turn(TurnState::Completed)]);
    let expected_projection_revision = projection(&fixture.connection);
    let prepared = prepare_turn(
        &mut fixture.connection,
        PrepareTurnCommand {
            project_id: owner.project_id.clone(),
            project_generation: ProjectGeneration::new(1).unwrap(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            expected_projection_revision,
            client_intent_id: ClientIntentId::parse(intent).unwrap(),
            prompt: Prompt::new("prompt", "next").unwrap(),
            now_millis: 9,
        },
    )
    .unwrap();
    begin_send(
        &mut fixture.connection,
        TurnMutationCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            turn_id: prepared.turn_id.clone(),
            expected_revision: prepared.turn_revision,
            now_millis: 10,
        },
    )
    .unwrap();
    let next = AdapterStoreOwner {
        turn_id: prepared.turn_id,
        ..owner.clone()
    };
    apply(fixture, &next, vec![turn(TurnState::Running)]);
    next
}

fn surface(fixture: &Fixture) -> SessionSurface {
    let json: String = fixture
        .connection
        .query_row("SELECT surface_json FROM acp_session_surfaces", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&json).unwrap()
}

#[test]
fn an_earlier_turns_rows_survive_newer_rows_and_a_new_epoch_reset() {
    let mut fixture = fixture();
    let first = fixture.owner.clone();
    apply(
        &mut fixture,
        &first,
        vec![
            turn(TurnState::Running),
            hook("lint", Running, "linting"),
            hook("lint", Failed, "Synthetic lint hook exited with status 1"),
        ],
    );
    let second = next_turn(&mut fixture, &first, "second-turn");
    for index in 0..20 {
        apply(
            &mut fixture,
            &second,
            vec![hook(&format!("newer-{index:02}"), Running, "-")],
        );
    }
    let live = surface(&fixture);
    assert!(
        live.activities
            .iter()
            .all(|row| row.label.as_str() != "lint"),
        "newer rows evicted the earlier Turn's row from the live surface"
    );
    let mut reset = live;
    reset.apply(2, &notice(SessionNoticeLevel::Info, "restarted"));
    assert!(reset.activities.is_empty());
    fixture
        .connection
        .execute(
            "UPDATE acp_session_surfaces SET surface_json=?1",
            [serde_json::to_string(&reset).unwrap()],
        )
        .unwrap();

    let turns = rows(&fixture);
    assert_eq!(
        turns[0],
        [row(
            "lint",
            "failed",
            "Synthetic lint hook exited with status 1"
        )]
    );
    assert_eq!(turns[1].len(), 16, "a Turn keeps at most the row cap");
    assert_eq!(turns[1][0], row("newer-00", "running", "-"));
}

#[test]
fn a_hook_that_settles_after_its_turn_ended_updates_that_turns_row() {
    let mut fixture = fixture();
    let first = fixture.owner.clone();
    apply(
        &mut fixture,
        &first,
        vec![
            turn(TurnState::Running),
            hook("format", Running, "formatting"),
        ],
    );
    apply(&mut fixture, &first, vec![hook("format", Cancelled, "-")]);
    let second = next_turn(&mut fixture, &first, "second-turn");
    apply(
        &mut fixture,
        &second,
        vec![late_hook("format", Completed), hook("guard", Running, "-")],
    );
    let turns = rows(&fixture);
    assert_eq!(turns[0], [row("format", "completed", "settled late")]);
    assert_eq!(
        turns[1],
        [row("guard", "running", "-")],
        "no duplicate row in the running Turn"
    );
}

#[test]
fn a_late_settle_before_the_next_turn_starts_stays_in_the_ended_turn() {
    let mut fixture = fixture();
    let owner = fixture.owner.clone();
    apply(
        &mut fixture,
        &owner,
        vec![
            turn(TurnState::Running),
            hook("stop", Running, "-"),
            turn(TurnState::Completed),
            late_hook("stop", Failed),
        ],
    );
    assert_eq!(rows(&fixture)[0], [row("stop", "failed", "settled late")]);
}

#[test]
fn a_rerunning_hook_stops_appending_at_the_turn_cap_but_still_settles() {
    let mut fixture = fixture();
    let owner = fixture.owner.clone();
    apply(&mut fixture, &owner, vec![turn(TurnState::Running)]);
    for run in 0..60 {
        apply(
            &mut fixture,
            &owner,
            vec![
                hook("pre", Running, &format!("run {run}")),
                hook("pre", Completed, &format!("run {run}")),
            ],
        );
    }
    let facts: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE kind='sessionActivity'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(facts, 64);
    assert_eq!(rows(&fixture)[0], [row("pre", "completed", "run 31")]);
}

#[test]
fn a_fact_that_does_not_decode_is_skipped_and_the_page_still_loads() {
    let mut fixture = fixture();
    let owner = fixture.owner.clone();
    apply(&mut fixture, &owner, vec![turn(TurnState::Running)]);
    {
        let revision = projection(&fixture.connection);
        let transaction = fixture.connection.transaction().unwrap();
        super::super::facts::append_fact_payload(
            &transaction,
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
            Some(owner.turn_id.as_str()),
            "sessionActivity",
            &json!({"i": "future", "shape": 2}).to_string(),
            revision.next().unwrap(),
            7,
        )
        .unwrap();
        transaction.commit().unwrap();
    }
    apply(&mut fixture, &owner, vec![hook("lint", Completed, "ok")]);
    assert_eq!(rows(&fixture)[0], [row("lint", "completed", "ok")]);
}

fn activity_facts(fixture: &Fixture) -> i64 {
    fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE kind='sessionActivity'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn past_the_turn_cap_a_cancelled_hook_still_settles_to_its_late_outcome() {
    let mut fixture = fixture();
    let owner = fixture.owner.clone();
    apply(
        &mut fixture,
        &owner,
        vec![turn(TurnState::Running), hook("guard", Running, "-")],
    );
    // 63 more facts: a rerunning hook ends running at the cap of 64.
    for run in 0..32 {
        let mut events = vec![hook("pre", Running, &format!("run {run}"))];
        if run < 31 {
            events.push(hook("pre", Completed, &format!("run {run}")));
        }
        apply(&mut fixture, &owner, events);
    }
    assert_eq!(activity_facts(&fixture), 64);
    apply(
        &mut fixture,
        &owner,
        vec![
            hook("guard", Cancelled, "-"),
            hook("guard", Running, "rerun"),
            hook("guard", Failed, "Guard exited with status 2"),
            hook("guard", Completed, "-"),
        ],
    );
    assert_eq!(activity_facts(&fixture), 66, "only the two settles");
    assert_eq!(
        rows(&fixture)[0][0],
        row("guard", "failed", "Guard exited with status 2")
    );
    for index in 0..16 {
        apply(
            &mut fixture,
            &owner,
            vec![hook("pre", Cancelled, &index.to_string())],
        );
        apply(
            &mut fixture,
            &owner,
            vec![hook("pre", Completed, &index.to_string())],
        );
    }
    assert_eq!(activity_facts(&fixture), 68);
}

#[test]
fn hook_rows_stop_short_of_the_sessions_fact_capacity() {
    let mut fixture = fixture();
    let owner = fixture.owner.clone();
    apply(&mut fixture, &owner, vec![turn(TurnState::Running)]);
    let headroom = LIMITS.durable.facts_per_session * 90 / 100;
    fixture
        .connection
        .execute(
            "UPDATE acp_session_usage SET fact_count=?1 WHERE session_id=?2",
            rusqlite::params![headroom as i64, owner.session_id.as_str()],
        )
        .unwrap();
    for state in [Running, Completed] {
        let result = apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(&owner, vec![hook("lint", state, "-")]),
        )
        .unwrap();
        assert_eq!(
            result.disposition,
            MutationDisposition::Applied,
            "the live surface still moves"
        );
    }
    assert_eq!(activity_facts(&fixture), 0);
    assert!(rows(&fixture)[0].is_empty());
}

#[test]
fn settles_past_the_turn_cap_stop_at_its_hard_ceiling() {
    let mut fixture = fixture();
    let owner = fixture.owner.clone();
    let names = (0..16)
        .map(|index| format!("hook-{index:02}"))
        .collect::<Vec<_>>();
    let mut events = vec![turn(TurnState::Running)];
    events.extend(names.iter().map(|name| hook(name, Running, "-")));
    apply(&mut fixture, &owner, events);
    // 48 more facts leave every row running at the cap of 64.
    for run in 0..24 {
        apply(
            &mut fixture,
            &owner,
            vec![
                hook(&names[0], Completed, &run.to_string()),
                hook(&names[0], Running, &run.to_string()),
            ],
        );
    }
    assert_eq!(activity_facts(&fixture), 64);
    for name in &names {
        apply(
            &mut fixture,
            &owner,
            vec![hook(name, Cancelled, "-"), hook(name, Failed, "late")],
        );
    }
    assert_eq!(activity_facts(&fixture), 80);
    let projected = rows(&fixture);
    assert_eq!(projected[0][0], row(&names[0], "failed", "late"));
    assert_eq!(projected[0][7], row(&names[7], "failed", "late"));
    assert_eq!(
        projected[0][8],
        row(&names[8], "running", "-"),
        "the ceiling was spent before this row settled"
    );
}
