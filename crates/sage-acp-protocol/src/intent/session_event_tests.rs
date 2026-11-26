use super::fixtures::title;
use super::*;

fn notice(text: &str) -> SessionScopedEvent {
    super::fixtures::notice(SessionNoticeLevel::Warning, text)
}

#[test]
fn one_problem_keeps_its_highest_severity_without_merging_different_problems() {
    let parse = |json: &str| serde_json::from_str::<SessionScopedEvent>(json).unwrap();
    let warning = parse(
        r#"{"kind":"notice","identity":"mcp-start:calendar","level":"warning","text":"Tools unavailable.","source":"runtime"}"#,
    );
    let failure = parse(
        r#"{"kind":"notice","identity":"mcp-start:calendar","level":"error","text":"Sign in to calendar.","source":"adapter"}"#,
    );
    let other = parse(
        r#"{"kind":"notice","identity":"mcp-start:mail","level":"warning","text":"Tools unavailable.","source":"runtime"}"#,
    );
    let mut surface = SessionSurface::default();
    surface.apply(1, &warning);
    surface.apply(1, &other);
    surface.apply(1, &failure);
    assert_eq!(surface.notices.len(), 2);
    assert_eq!(surface.notices[0].level, SessionNoticeLevel::Error);
    assert_eq!(surface.notices[0].text.as_str(), "Sign in to calendar.");
    assert_eq!(surface.apply(1, &warning), None);
    assert_eq!(surface.notices[1].text.as_str(), "Tools unavailable.");
}

#[test]
fn a_usage_window_keeps_only_its_latest_report_and_never_stores_a_bare_all_clear() {
    let parse = |level: &str, used: u8| {
        serde_json::from_str::<SessionScopedEvent>(&format!(
            r#"{{"kind":"notice","identity":"usage-limit:five_hour","level":"{level}","text":"Claude 5-hour limit.","source":"claude code","usageLimit":{{"provider":"Claude","usedPercent":{used},"windowMinutes":300,"resetsAt":1795600000}}}}"#
        ))
        .unwrap()
    };
    let mut surface = SessionSurface::default();
    assert_eq!(surface.apply(1, &parse("info", 10)), None);
    assert!(surface.notices.is_empty());
    surface.apply(1, &parse("error", 100));
    surface.apply(1, &parse("warning", 80));
    assert_eq!(surface.notices.len(), 1);
    assert_eq!(surface.notices[0].level, SessionNoticeLevel::Warning);
    assert_eq!(
        surface.notices[0]
            .usage_limit
            .as_ref()
            .unwrap()
            .used_percent,
        Some(80)
    );
    surface.apply(1, &parse("info", 5));
    assert_eq!(surface.notices.len(), 1);
    assert_eq!(surface.notices[0].level, SessionNoticeLevel::Info);
    assert_eq!(surface.apply(1, &parse("info", 9)), None);
    assert_eq!(
        surface.notices[0]
            .usage_limit
            .as_ref()
            .unwrap()
            .used_percent,
        Some(5)
    );
}

fn activity(id: &str, state: SessionActivityState, progress: &str) -> SessionScopedEvent {
    SessionScopedEvent::Activity(super::fixtures::activity(id, state, progress))
}

#[test]
fn replayed_legacy_actionable_warning_stays_visible() {
    let legacy: SessionScopedEvent = serde_json::from_str(
        r#"{"kind":"notice","level":"warning","text":"The MCP server calendar needs you to sign in again; its tools are unavailable until then.","source":"codex mcp"}"#,
    ).unwrap();
    let mut surface = SessionSurface::default();
    surface.apply(1, &legacy);
    assert_eq!(surface.notices.len(), 1);
    assert!(!surface.notices[0].diagnostic);
    assert_eq!(
        surface.notices[0].text.as_str(),
        "The MCP server calendar needs you to sign in again; its tools are unavailable until then."
    );
    let current = super::fixtures::notice(SessionNoticeLevel::Warning, "Approval needs attention");
    let encoded = serde_json::to_value(&current).unwrap();
    assert_eq!(encoded["diagnostic"], false);
    assert_eq!(
        serde_json::from_value::<SessionScopedEvent>(encoded).unwrap(),
        current
    );
}

#[test]
fn notices_deduplicate_and_keep_only_the_newest() {
    let mut surface = SessionSurface::default();
    assert_eq!(
        surface.apply(1, &notice("same")),
        Some(SurfaceChange::SendNeutral)
    );
    assert_eq!(surface.apply(1, &notice("same")), None);
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
        assert_eq!(
            surface.apply(
                1,
                &activity("task", SessionActivityState::Running, progress)
            ),
            Some(SurfaceChange::SendNeutral)
        );
    }
    assert_eq!(
        surface.apply(1, &activity("task", SessionActivityState::Running, "3/3")),
        None
    );
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

    assert_eq!(
        surface.apply(2, &notice("deprecated flag")),
        Some(SurfaceChange::SendRelevant)
    );
    assert_eq!(surface.title.as_ref().unwrap().as_str(), "Fix the sidebar");
    assert_eq!(surface.notices.len(), 1);
    assert_eq!(surface.live_epoch, 2);
    assert!(surface.model_id.is_none());
    assert!(surface.activities.is_empty());
    assert_eq!(surface.apply(2, &title("Fix the sidebar")), None);
}

#[test]
fn a_new_epoch_without_a_model_resets_activity_as_a_neutral_change() {
    let mut surface = SessionSurface::default();
    surface.apply(1, &activity("task", SessionActivityState::Running, "1/3"));
    assert_eq!(
        surface.apply(2, &title("Next epoch")),
        Some(SurfaceChange::SendNeutral)
    );
    assert!(surface.activities.is_empty());
}

#[test]
fn eviction_drops_notices_then_finished_then_running_activity() {
    let mut surface = SessionSurface::default();
    surface.apply(1, &activity("running", SessionActivityState::Running, "-"));
    surface.apply(1, &activity("done", SessionActivityState::Completed, "-"));
    surface.apply(1, &notice("old"));
    surface.apply(1, &title("kept"));

    assert!(surface.evict_oldest());
    assert!(surface.notices.is_empty());
    assert!(surface.evict_oldest());
    assert_eq!(surface.activities[0].id.as_str(), "running");
    assert!(surface.evict_oldest());
    assert!(surface.activities.is_empty());
    assert!(!surface.evict_oldest());
    assert_eq!(surface.title.as_ref().unwrap().as_str(), "kept");
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

#[test]
fn a_chat_sees_live_activities_by_key_and_only_running_ones_can_be_stopped() {
    let mut surface = SessionSurface::default();
    let mut running = fixtures::activity("task:a", SessionActivityState::Running, "working");
    running.stoppable = true;
    let mut done = fixtures::activity("task:b", SessionActivityState::Completed, "done");
    done.stoppable = true;
    surface.apply(2, &SessionScopedEvent::Activity(running.clone()));
    surface.apply(2, &SessionScopedEvent::Activity(done));
    let view = surface.view(Some(2));
    assert_eq!(view.activities.len(), 2);
    assert!(view.activities[0].stoppable);
    assert!(!view.activities[1].stoppable);
    let key = &view.activities[0].key;
    assert_eq!(key.len(), 24);
    assert!(!key.contains("task"));
    assert_eq!(surface.activity_id(key), Some(&running.id));
    assert_eq!(surface.activity_id("unknown"), None);
    assert!(surface.view(Some(3)).activities.is_empty());
    assert!(surface.view(None).activities.is_empty());
}

#[test]
fn a_cleared_title_replaces_the_title_and_a_repeat_clear_is_no_change() {
    let cleared = SessionScopedEvent::Title { title: None };
    let mut surface = SessionSurface::default();
    assert_eq!(surface.apply(1, &cleared), None);
    surface.apply(1, &title("Fix the sidebar"));
    assert_eq!(surface.apply(1, &cleared), Some(SurfaceChange::SendNeutral));
    assert_eq!(surface.title, None);
    assert_eq!(surface.apply(1, &cleared), None);
    assert_eq!(
        surface.apply(1, &title("Fix the sidebar")),
        Some(SurfaceChange::SendNeutral)
    );

    let mut pending = PendingSessionEvents::default();
    pending.push(title("first"));
    pending.push(cleared.clone());
    assert_eq!(pending.take(), [cleared]);
}

#[test]
fn chrome_lives_only_in_its_epoch_and_is_evicted_before_notices() {
    let chrome: SessionScopedEvent = serde_json::from_str(
        r#"{"kind":"chrome","statuses":[{"key":"demo","text":"Turn 1"}],
            "widgets":[{"key":"demo","placement":"below","lines":["Line"],"hiddenLines":2}],
            "editorText":{"revision":3,"text":"Suggested"},"subtitle":"Demo"}"#,
    )
    .unwrap();
    let mut surface = SessionSurface::default();
    assert_eq!(surface.apply(1, &chrome), Some(SurfaceChange::SendNeutral));
    assert_eq!(surface.apply(1, &chrome), None);
    let view = serde_json::to_value(surface.view(Some(1))).unwrap();
    assert_eq!(view["chrome"]["editorText"]["key"], "1-3");
    assert_eq!(view["chrome"]["widgets"][0]["hiddenLines"], 2);
    // A session that is not live, or another epoch, shows no chrome.
    assert!(surface.view(None).chrome.is_none());
    assert!(surface.view(Some(2)).chrome.is_none());
    // The offer, then the widgets, give way before any notice.
    surface.apply(1, &notice("kept"));
    assert!(surface.evict_oldest());
    assert!(surface.chrome.editor_text.is_none());
    assert!(surface.evict_oldest());
    assert!(surface.chrome.widgets.as_slice().is_empty());
    assert_eq!(surface.chrome.statuses.as_slice().len(), 1);
    assert_eq!(surface.notices.len(), 1);
    // A new epoch drops the old epoch's chrome as a neutral change.
    assert_eq!(
        surface.apply(2, &title("Next")),
        Some(SurfaceChange::SendNeutral)
    );
    assert!(surface.chrome.is_empty());
}
