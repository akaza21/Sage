use super::*;
use crate::acp::intent::session_event::{
    SessionActivityState, SessionNoticeLevel, SessionScopedEvent,
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

fn surface(fixture: &Fixture) -> crate::acp::projection::SessionSurfaceProjection {
    load_chat_page(
        &fixture.connection,
        fixture.owner.project_id.clone(),
        fixture.owner.chat_id.clone(),
        None,
    )
    .unwrap()
    .session_surface
    .expect("current session surface")
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

    let projected = surface(&fixture);
    assert_eq!(projected.session_id, fixture.owner.session_id);
    assert_eq!(projected.title.as_deref(), Some("Fix the sidebar"));
    assert_eq!(projected.notices.len(), 1);
    assert_eq!(projected.notices[0].text, "config deprecated");

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
    let projected = surface(&fixture);
    assert_eq!(rows, 1);
    assert_eq!(projected.notices.len(), 16);
    assert_eq!(projected.notices[0].text, "notice 24");
    assert_eq!(projected.notices[15].text, "notice 39");
}

#[test]
fn live_activity_and_model_show_only_for_the_running_epoch() {
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
    let live = surface(&fixture);
    assert_eq!(
        live.model_id.as_ref().map(|id| id.as_str()),
        Some("rerouted")
    );
    assert_eq!(live.activities.len(), 1);
    assert_eq!(live.activities[0].progress.as_deref(), Some("12 of 40"));
    assert_eq!(live.activities[0].key.len(), 64);
    assert!(!live.activities[0].key.contains("native-task-7"));

    fixture
        .connection
        .execute(
            "UPDATE session_runtime_epochs SET state='exited' WHERE session_id=?1",
            [fixture.owner.session_id.as_str()],
        )
        .unwrap();
    let cold = surface(&fixture);
    assert!(cold.model_id.is_none());
    assert!(cold.activities.is_empty());
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
