use super::*;

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
