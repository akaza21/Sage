use super::*;
use sage_acp_protocol::intent::session_event::{
    SESSION_NOTICES_MAX, SessionActivityState, SessionNoticeLevel, SessionScopedEvent,
    SessionSurface,
    fixtures::{activity as activity_fixture, notice as notice_fixture, title},
};

fn session(event: SessionScopedEvent, now_millis: i64) -> AdapterInboundEvent {
    AdapterInboundEvent::Session(SessionEventCommand { event, now_millis })
}

fn notice(text: &str) -> SessionScopedEvent {
    notice_fixture(SessionNoticeLevel::Warning, text)
}

#[test]
fn problem_identity_survives_durable_notice_reload() {
    let mut fixture = fixture();
    let event: SessionScopedEvent = serde_json::from_str(
        r#"{"kind":"notice","identity":"mcp-start:calendar","level":"warning","text":"Tools unavailable.","source":"runtime"}"#,
    ).unwrap();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![session(event, 6)]),
    )
    .unwrap();
    let reloaded = crate::acp::store::load_chat_page(
        &fixture.connection,
        fixture.owner.project_id.clone(),
        fixture.owner.chat_id.clone(),
        None,
    )
    .unwrap();
    assert_eq!(
        reloaded.session_notices[0]
            .identity
            .as_ref()
            .unwrap()
            .as_str(),
        "mcp-start:calendar"
    );
}

#[test]
fn unclassified_and_legacy_notice_identities_stay_stable_as_the_surface_changes() {
    let mut fixture = fixture();
    let apply = |fixture: &mut Fixture, text| {
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(&fixture.owner, vec![session(notice(text), 6)]),
        )
        .unwrap();
    };
    apply(&mut fixture, "Unclassified notice");
    let initial = surface(&fixture).notices[0].identity.clone().unwrap();
    apply(&mut fixture, "Unclassified notice");
    assert_eq!(surface(&fixture).notices.len(), 1);
    assert_eq!(
        surface(&fixture).notices[0].identity.as_ref(),
        Some(&initial)
    );
    let mut legacy = surface(&fixture);
    legacy.notices[0].identity = None;
    fixture
        .connection
        .execute(
            "UPDATE acp_session_surfaces SET surface_json=?1",
            [serde_json::to_string(&legacy).unwrap()],
        )
        .unwrap();
    let replayed = surface(&fixture).notices[0].identity.clone().unwrap();
    apply(&mut fixture, "A separate problem");
    let stored = surface(&fixture);
    assert_eq!(stored.notices[0].identity.as_ref(), Some(&replayed));
    assert_ne!(stored.notices[0].identity, stored.notices[1].identity);
}

fn turn(state: TurnState, now_millis: i64) -> AdapterInboundEvent {
    AdapterInboundEvent::Turn {
        native_turn_id: None,
        state,
        now_millis,
    }
}

fn surface(fixture: &Fixture) -> SessionSurface {
    super::super::session_surface::read(
        &fixture.connection,
        fixture.owner.project_id.as_str(),
        fixture.owner.chat_id.as_str(),
        fixture.owner.session_id.as_str(),
    )
    .unwrap()
    .expect("stored session surface")
    .0
}

#[test]
fn session_facts_persist_after_the_turn_terminal_and_reload_with_the_chat() {
    let mut fixture = fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                turn(TurnState::Running, 6),
                session(title("Fix the sidebar"), 6),
                turn(TurnState::Completed, 7),
            ],
        ),
    )
    .unwrap();
    let before = projection(&fixture.connection);
    let after_terminal = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![session(notice("config deprecated"), 8)],
        ),
    )
    .unwrap();
    assert_eq!(after_terminal.disposition, MutationDisposition::Applied);
    assert_eq!(after_terminal.turn_cursor().state(), TurnState::Completed);
    assert!(after_terminal.projection_revision > before);

    let stored = surface(&fixture);
    assert_eq!(
        stored.title.as_ref().map(|title| title.as_str()),
        Some("Fix the sidebar")
    );
    assert_eq!(stored.notices.len(), 1);
    assert_eq!(stored.notices[0].text.as_str(), "config deprecated");

    let repeated = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![session(notice("config deprecated"), 9)],
        ),
    )
    .unwrap();
    assert_eq!(repeated.disposition, MutationDisposition::Duplicate);
}

#[test]
fn storage_is_one_bounded_row_per_session() {
    let mut fixture = fixture();
    for index in 0..40 {
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(
                &fixture.owner,
                vec![session(notice(&format!("notice {index}")), 6 + index)],
            ),
        )
        .unwrap();
    }
    let rows: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM acp_session_surfaces", [], |row| {
            row.get(0)
        })
        .unwrap();
    let stored = surface(&fixture);
    assert_eq!(rows, 1);
    assert_eq!(stored.notices.len(), 16);
    assert_eq!(stored.notices[0].text.as_str(), "notice 24");
    assert_eq!(stored.notices[15].text.as_str(), "notice 39");
}

#[test]
fn the_chat_page_carries_the_current_sessions_newest_notices() {
    let mut fixture = fixture();
    let page = |fixture: &Fixture| {
        crate::acp::store::load_chat_page(
            &fixture.connection,
            fixture.owner.project_id.clone(),
            fixture.owner.chat_id.clone(),
            None,
        )
        .unwrap()
    };
    assert!(page(&fixture).session_notices.is_empty());
    for index in 0..20 {
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(
                &fixture.owner,
                vec![session(notice(&format!("notice {index}")), 6 + index)],
            ),
        )
        .unwrap();
    }
    let notices = page(&fixture).session_notices;
    assert_eq!(notices.len(), SESSION_NOTICES_MAX);
    assert_eq!(notices[0].text.as_str(), "notice 4");
    assert_eq!(notices[SESSION_NOTICES_MAX - 1].text.as_str(), "notice 19");
    assert_eq!(notices[0].level, SessionNoticeLevel::Warning);
}

#[test]
fn escape_heavy_notices_evict_old_rows_instead_of_failing_the_batch() {
    let mut fixture = fixture();
    // Each control byte serializes as six JSON bytes, so sixteen full
    // notices would be far over the row cap.
    let text = |index: usize| format!("{}{index:02}", "\u{1}".repeat(1020));
    for index in 0..SESSION_NOTICES_MAX {
        let result = apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(&fixture.owner, vec![session(notice(&text(index)), 6)]),
        )
        .unwrap();
        assert_eq!(result.disposition, MutationDisposition::Applied);
    }
    let bytes: i64 = fixture
        .connection
        .query_row(
            "SELECT length(CAST(surface_json AS BLOB)) FROM acp_session_surfaces",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let stored = surface(&fixture);
    assert!(bytes <= 65_536);
    assert!(stored.notices.len() < SESSION_NOTICES_MAX);
    assert_eq!(
        stored.notices.last().unwrap().text.as_str(),
        text(SESSION_NOTICES_MAX - 1)
    );
}

#[test]
fn live_activity_and_model_are_stored_for_the_reporting_epoch() {
    let mut fixture = fixture();
    let activity = activity_fixture("native-task-7", SessionActivityState::Running, "12 of 40");
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                session(SessionScopedEvent::Activity(activity), 6),
                session(
                    SessionScopedEvent::TurnModel {
                        model_id: sage_acp_protocol::intent::ModelId::new("modelId", "rerouted")
                            .unwrap(),
                    },
                    6,
                ),
            ],
        ),
    )
    .unwrap();
    let stored = surface(&fixture);
    assert_eq!(stored.live_epoch, fixture.owner.runtime_epoch.get());
    assert_eq!(
        stored.model_id.as_ref().map(|id| id.as_str()),
        Some("rerouted")
    );
    assert_eq!(stored.activities.len(), 1);
    assert_eq!(
        stored.activities[0].progress.as_ref().map(|p| p.as_str()),
        Some("12 of 40")
    );
}

#[test]
fn a_foreign_owner_cannot_write_the_session_surface() {
    let mut fixture = fixture();
    let mut foreign = fixture.owner.clone();
    foreign.runtime_epoch = RuntimeEpoch::new(2).unwrap();
    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(&foreign, vec![session(notice("stale epoch"), 6)]),
        ),
        Err(StoreError::Conflict {
            resource: "adapterBatchOwner"
        })
    ));
    let rows: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM acp_session_surfaces", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn only_the_latest_finished_turn_of_its_session_can_be_undone() {
    let mut fixture = fixture();
    let undoable = |fixture: &Fixture, session: &str, turn: &str| {
        super::super::session_surface::is_undoable_turn(
            &fixture.connection,
            fixture.owner.project_id.as_str(),
            fixture.owner.chat_id.as_str(),
            session,
            turn,
        )
        .unwrap()
    };
    let session = fixture.owner.session_id.as_str().to_owned();
    let turn_id = fixture.owner.turn_id.as_str().to_owned();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![turn(TurnState::Running, 6)]),
    )
    .unwrap();
    assert!(!undoable(&fixture, &session, &turn_id), "still running");
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![turn(TurnState::Completed, 7)]),
    )
    .unwrap();
    assert!(undoable(&fixture, &session, &turn_id));
    assert!(!undoable(&fixture, "session-other", &turn_id));
    assert!(!undoable(&fixture, &session, "turn-other"));
}
