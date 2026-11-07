use super::fixtures::title;
use super::*;

fn notice(text: &str) -> SessionScopedEvent {
    super::fixtures::notice(SessionNoticeLevel::Warning, text)
}

fn activity(id: &str, state: SessionActivityState, progress: &str) -> SessionScopedEvent {
    SessionScopedEvent::Activity(super::fixtures::activity(id, state, progress))
}

#[test]
fn notices_deduplicate_and_keep_only_the_newest() {
    let mut surface = SessionSurface::default();
    assert!(surface.apply(1, &notice("same")));
    assert!(!surface.apply(1, &notice("same")));
    for index in 0..SESSION_NOTICES_MAX + 4 {
        surface.apply(1, &notice(&format!("notice {index}")));
    }
    assert_eq!(surface.notices.len(), SESSION_NOTICES_MAX);
    assert_eq!(surface.notices[0].text.as_str(), "notice 4");
    assert!(surface.is_bounded());
}

#[test]
fn activity_updates_replace_their_row_and_eviction_prefers_terminal_rows() {
    let mut surface = SessionSurface::default();
    for progress in ["1/3", "2/3", "3/3"] {
        assert!(surface.apply(
            1,
            &activity("task", SessionActivityState::Running, progress)
        ));
    }
    assert!(!surface.apply(1, &activity("task", SessionActivityState::Running, "3/3")));
    assert_eq!(surface.activities.len(), 1);
    assert_eq!(
        surface.activities[0].progress.as_ref().unwrap().as_str(),
        "3/3"
    );

    surface.apply(
        1,
        &activity("task", SessionActivityState::Completed, "done"),
    );
    for index in 1..SESSION_ACTIVITIES_MAX {
        surface.apply(
            1,
            &activity(&format!("run-{index}"), SessionActivityState::Running, "-"),
        );
    }
    surface.apply(1, &activity("late", SessionActivityState::Running, "-"));
    assert_eq!(surface.activities.len(), SESSION_ACTIVITIES_MAX);
    assert!(
        surface
            .activities
            .iter()
            .all(|row| row.id.as_str() != "task")
    );
    surface.apply(1, &activity("later", SessionActivityState::Running, "-"));
    assert_eq!(surface.activities[0].id.as_str(), "run-2");
}

#[test]
fn a_new_runtime_epoch_resets_live_state_but_keeps_history() {
    let mut surface = SessionSurface::default();
    surface.apply(1, &title("Fix the sidebar"));
    surface.apply(1, &notice("deprecated flag"));
    surface.apply(
        1,
        &SessionScopedEvent::TurnModel {
            model_id: ModelId::new("modelId", "gpt-mini").unwrap(),
        },
    );
    surface.apply(1, &activity("task", SessionActivityState::Running, "1/3"));

    assert!(surface.apply(2, &notice("deprecated flag")));
    assert_eq!(surface.title.as_ref().unwrap().as_str(), "Fix the sidebar");
    assert_eq!(surface.notices.len(), 1);
    assert_eq!(surface.live_epoch, 2);
    assert!(surface.model_id.is_none());
    assert!(surface.activities.is_empty());
    assert!(!surface.apply(2, &title("Fix the sidebar")));
}

#[test]
fn stored_surface_round_trips_and_rejects_unknown_fields() {
    let mut surface = SessionSurface::default();
    surface.apply(3, &notice("watch out"));
    let json = serde_json::to_string(&surface).unwrap();
    assert_eq!(
        serde_json::from_str::<SessionSurface>(&json).unwrap(),
        surface
    );
    assert!(
        serde_json::from_str::<SessionSurface>(
            r#"{"title":null,"notices":[],"liveEpoch":1,"modelId":null,"activities":[],"extra":1}"#
        )
        .is_err()
    );
}

#[test]
fn the_pre_turn_buffer_replaces_superseded_events_and_drops_the_oldest_at_its_cap() {
    let mut pending = PendingSessionEvents::default();
    pending.push(title("first"));
    for index in 0..SESSION_PENDING_EVENTS_MAX + 3 {
        pending.push(notice(&format!("notice {index}")));
    }
    pending.push(title("second"));
    let events = pending.take();
    assert_eq!(events.len(), SESSION_PENDING_EVENTS_MAX);
    assert_eq!(events[0], notice("notice 4"));
    assert_eq!(events.last(), Some(&title("second")));
    assert!(pending.take().is_empty());
}
