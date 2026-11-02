use super::*;
use crate::workspace::notify::unix_events::{UnixWatchKind, unix_watch_delivery};

static WATCH_TEST_LOCK: Mutex<()> = Mutex::new(());

fn resource(relative_path: &str) -> ResourceRef {
    ResourceRef {
        root_id: "root-a".into(),
        relative_path: relative_path.into(),
    }
}

#[test]
fn watched_file_events_emit_changed_or_deleted_and_ignore_siblings() {
    assert_eq!(
        notice_kind_for_path("notes.md", "notes.md", FILE_ACTION_MODIFIED),
        Some(EditorExternalWatchKind::Changed)
    );
    assert_eq!(
        notice_kind_for_path("notes.md", "notes.md", FILE_ACTION_REMOVED),
        Some(EditorExternalWatchKind::Deleted)
    );
    assert_eq!(
        notice_kind_for_path("notes.md", "sibling.md", FILE_ACTION_MODIFIED),
        None
    );
}

#[test]
fn cancelled_watches_cannot_emit_and_generations_do_not_repeat() {
    let _guard = WATCH_TEST_LOCK.lock().unwrap();
    let file = resource("notes.md");
    watches().clear();
    generations().clear();
    let first = start_watch(&file).unwrap();
    let cancelled = watch_control(&file, first).unwrap();
    assert!(can_emit(&file, first, &cancelled));
    let mut delivered = Vec::new();
    deliver_if_active(
        &file,
        first,
        &cancelled,
        EditorExternalWatchKind::Changed,
        |notice| delivered.push(notice),
    );
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].kind, EditorExternalWatchKind::Changed);
    assert_eq!(watch_event_name(), "editor-external-watch-v1");
    stop_watch(&file).unwrap();
    assert!(!can_emit(&file, first, &cancelled));
    deliver_if_active(
        &file,
        first,
        &cancelled,
        EditorExternalWatchKind::Deleted,
        |notice| delivered.push(notice),
    );
    assert_eq!(delivered.len(), 1);
    let second = start_watch(&file).unwrap();
    assert!(second > first);
    finish_watch(&file, second);
    assert_eq!(start_watch(&file).unwrap(), second + 1);
    stop_watch(&file).unwrap();
}

#[test]
fn global_watch_limit_rejects_overflow_and_releases_stopped_slots() {
    let _guard = WATCH_TEST_LOCK.lock().unwrap();
    watches().clear();
    generations().clear();
    let first = resource("first.md");
    let first_generation = start_watch(&first).unwrap();
    let first_cancelled = watch_control(&first, first_generation).unwrap();
    assert!(matches!(
        start_watch(&first),
        Err(AppError::InvalidRequest(_))
    ));

    let mut active = vec![first.clone()];
    for index in 1..MAX_GLOBAL_WATCHES {
        let file = ResourceRef {
            root_id: format!("root-{index}"),
            relative_path: "notes.md".into(),
        };
        start_watch(&file).unwrap();
        active.push(file);
    }
    let overflow = resource("overflow.md");
    assert_eq!(
        start_watch(&overflow).map_err(watch_command_error),
        Err(EDITOR_WATCH_LIMIT_ERROR.to_string())
    );
    assert_eq!(
        start_watch(&first).map_err(watch_command_error),
        Err("invalid request: a watch is already active for this resource".to_string())
    );
    assert!(can_emit(&first, first_generation, &first_cancelled));

    stop_watch(&first).unwrap();
    assert!(first_cancelled.load(Ordering::Acquire));
    let generation = start_watch(&overflow).unwrap();
    assert!(matches!(
        start_watch(&first),
        Err(AppError::ResourceLimit(_))
    ));
    stop_watch(&overflow).unwrap();
    let next_generation = start_watch(&first).unwrap();
    assert!(next_generation > first_generation);
    finish_watch(&first, first_generation);
    assert!(matches!(
        start_watch(&overflow),
        Err(AppError::ResourceLimit(_))
    ));
    finish_watch(&first, next_generation);
    assert!(start_watch(&overflow).unwrap() > generation);

    stop_watch(&overflow).unwrap();
    for file in active {
        stop_watch(&file).unwrap();
    }
}

#[test]
fn unix_named_and_overflow_deliveries_map_to_editor_kinds() {
    let changed =
        unix_watch_delivery(Some("notes.md"), UnixWatchKind::Changed).expect("named change");
    assert_eq!(
        editor_notice_kind_from_unix_delivery("notes.md", &changed.0, changed.1),
        Some(EditorExternalWatchKind::Changed)
    );

    let deleted =
        unix_watch_delivery(Some("notes.md"), UnixWatchKind::Deleted).expect("named delete");
    assert_eq!(
        editor_notice_kind_from_unix_delivery("notes.md", &deleted.0, deleted.1),
        Some(EditorExternalWatchKind::Deleted)
    );

    let renamed_old =
        unix_watch_delivery(Some("notes.md"), UnixWatchKind::RenamedOld).expect("rename old");
    assert_eq!(
        editor_notice_kind_from_unix_delivery("notes.md", &renamed_old.0, renamed_old.1),
        Some(EditorExternalWatchKind::Deleted)
    );

    let sibling = unix_watch_delivery(Some("sibling.md"), UnixWatchKind::Changed).expect("sibling");
    assert_eq!(
        editor_notice_kind_from_unix_delivery("notes.md", &sibling.0, sibling.1),
        None
    );

    let overflow = unix_watch_delivery(None, UnixWatchKind::Changed).expect("overflow");
    assert_eq!(
        editor_notice_kind_from_unix_delivery("notes.md", &overflow.0, overflow.1),
        Some(EditorExternalWatchKind::Changed)
    );
}

#[test]
fn unix_file_names_are_case_sensitive_on_the_shared_contract() {
    let changed =
        unix_watch_delivery(Some("Notes.md"), UnixWatchKind::Changed).expect("case sibling");
    assert_eq!(
        editor_notice_kind_from_unix_delivery_case("notes.md", &changed.0, changed.1, true),
        None
    );
    assert_eq!(
        editor_notice_kind_from_unix_delivery_case("notes.md", &changed.0, changed.1, false),
        Some(EditorExternalWatchKind::Changed)
    );
}

#[test]
fn watch_run_failure_resyncs_unless_cancelled() {
    assert_eq!(
        editor_watch_failure_resync(false),
        Some(EditorExternalWatchKind::Changed)
    );
    assert_eq!(editor_watch_failure_resync(true), None);
}

#[test]
fn editor_unix_watch_is_macos_or_linux_only() {
    assert_eq!(
        editor_unix_watch_supported(),
        cfg!(any(target_os = "macos", target_os = "linux"))
    );
}
