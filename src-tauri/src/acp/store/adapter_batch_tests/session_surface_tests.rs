use super::*;
use crate::acp::intent::session_event::{
    SessionActivityState, SessionNoticeLevel, SessionScopedEvent, SessionSurface,
    fixtures::{activity as activity_fixture, notice as notice_fixture, title},
};

fn session(event: SessionScopedEvent, now_millis: i64) -> AdapterInboundEvent {
    AdapterInboundEvent::Session(SessionEventCommand { event, now_millis })
}

fn notice(text: &str) -> SessionScopedEvent {
    notice_fixture(SessionNoticeLevel::Warning, text)
}

fn turn(state: TurnState, now_millis: i64) -> AdapterInboundEvent {
    AdapterInboundEvent::Turn { state, now_millis }
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
                        model_id: crate::acp::intent::ModelId::new("modelId", "rerouted").unwrap(),
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
