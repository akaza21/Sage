use super::*;
use crate::acp::lifecycle::ItemLifecycle;

#[cfg(windows)]
struct TempRoot(std::path::PathBuf);

#[cfg(windows)]
struct LiveMapper(crate::acp::resource_mapper::NativeResourceMapper);

#[cfg(windows)]
impl ResourceMapper for LiveMapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        crate::acp::bindings::adapter::ResourceMapper::map_existing(&self.0, absolute_path)
    }

    fn map_missing_display(&self, absolute_path: &str) -> Option<SafeResourceRef> {
        crate::acp::bindings::adapter::ResourceMapper::map_missing_display(&self.0, absolute_path)
    }
}

#[cfg(windows)]
impl TempRoot {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("sage-codex-created-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

#[cfg(windows)]
impl Drop for TempRoot {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[cfg(windows)]
fn live_mapper(root: &TempRoot) -> LiveMapper {
    use crate::acp::resource_mapper::{ConfiguredResourceRoot, NativeResourceMapper};

    LiveMapper(
        NativeResourceMapper::new([ConfiguredResourceRoot {
            root_id: SageRootId::parse("root-created").unwrap(),
            canonical_native_root: root.0.clone(),
        }])
        .unwrap(),
    )
}

#[cfg(windows)]
fn active_adapter(mapper: &LiveMapper) -> CodexAdapter {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-file-change");
    adapter
        .ingest(
            &serde_json::to_vec(&json!({"method":"turn/started","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
            }}))
            .unwrap(),
            mapper,
        )
        .unwrap();
    adapter
}

#[test]
fn completed_file_change_notification_has_journal_compatible_shape() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-file-change");
    let turn = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&turn).unwrap(), &Mapper)
        .unwrap();

    let file = json!({"method":"item/completed","params":{
        "threadId":"thread-1","turnId":"turn-1","item":{
            "type":"fileChange","id":"file-1","status":"completed",
            "changes":[{"path":"C:\\repo\\src\\lib.rs","kind":{"type":"update"},"diff":"opaque"}]
        }
    }});
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&file).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!("expected file change event");
    };
    assert!(matches!(
        events.as_slice(),
        [
            IncomingEvent::Item {
                payload: NormalizedPayload::ToolCall {
                    category: crate::acp::intent::ToolCategory::Edit,
                    label,
                    ..
                },
                tool_state: Some(crate::acp::lifecycle::ToolState::Completed),
                lifecycle: ItemLifecycle::Completed,
                ..
            },
            IncomingEvent::Item {
                payload: NormalizedPayload::FileChangeDiff { .. },
                tool_state: None,
                ..
            },
        ] if label.as_str() == "Edited src/lib.rs"
    ));
}

#[test]
fn a_declined_or_failed_file_change_settles_its_diff_without_an_edit_row() {
    for (status, settled) in [
        ("declined", ItemLifecycle::Cancelled),
        ("failed", ItemLifecycle::Failed),
    ] {
        let mut adapter = adapter();
        ready(&mut adapter);
        arm_turn(&mut adapter, 2, "client-file-change");
        let turn = json!({"method":"turn/started","params":{
            "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
        }});
        adapter
            .ingest(&serde_json::to_vec(&turn).unwrap(), &Mapper)
            .unwrap();
        let file = json!({"method":"item/completed","params":{
            "threadId":"thread-1","turnId":"turn-1","item":{
                "type":"fileChange","id":"file-1","status":status,
                "changes":[{"path":"C:\\repo\\src\\lib.rs","kind":{"type":"update"},"diff":"opaque"}]
            }
        }});
        let IngressDisposition::Events(events) = adapter
            .ingest(&serde_json::to_vec(&file).unwrap(), &Mapper)
            .unwrap()
        else {
            panic!("expected file change event");
        };
        assert!(
            matches!(
                events.as_slice(),
                [IncomingEvent::Item {
                    lifecycle,
                    payload: NormalizedPayload::FileChangeDiff { .. },
                    ..
                }] if *lifecycle == settled
            ),
            "{status}: {events:?}"
        );
    }
}

#[test]
fn a_file_change_id_too_long_for_the_edit_row_still_settles_its_diff() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-file-change");
    let turn = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&turn).unwrap(), &Mapper)
        .unwrap();
    let file = json!({"method":"item/completed","params":{
        "threadId":"thread-1","turnId":"turn-1","item":{
            "type":"fileChange","id":"f".repeat(510),"status":"completed",
            "changes":[{"path":"C:\\repo\\src\\lib.rs","kind":{"type":"update"},"diff":"opaque"}]
        }
    }});
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&file).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!("expected file change event");
    };
    assert!(matches!(
        events.as_slice(),
        [IncomingEvent::Item {
            payload: NormalizedPayload::FileChangeDiff { .. },
            ..
        }]
    ));
}

#[cfg(windows)]
#[test]
fn created_file_becomes_openable_when_completed_after_disk_write() {
    let root = TempRoot::new();
    let mapper = live_mapper(&root);
    let path = root.0.join("cp8-pane-probe.md");
    let absolute = path.to_str().unwrap();
    let mut adapter = active_adapter(&mapper);

    let file = |method, status| {
        json!({"method":method,"params":{
            "threadId":"thread-1","turnId":"turn-1","item":{
                "type":"fileChange","id":"file-created","status":status,
                "changes":[{"path":absolute,"kind":{"type":"add"},"diff":"probe\n"}]
            }
        }})
    };
    let IngressDisposition::Events(started) = adapter
        .ingest(
            &serde_json::to_vec(&file("item/started", "inProgress")).unwrap(),
            &mapper,
        )
        .unwrap()
    else {
        panic!("expected started file change");
    };
    assert!(started.is_empty());

    std::fs::write(&path, "probe\n").unwrap();
    let IngressDisposition::Events(completed) = adapter
        .ingest(
            &serde_json::to_vec(&file("item/completed", "completed")).unwrap(),
            &mapper,
        )
        .unwrap()
    else {
        panic!("expected completed file change");
    };
    assert!(matches!(completed.as_slice(), [IncomingEvent::Item {
        payload: NormalizedPayload::ToolCall { category: crate::acp::intent::ToolCategory::Edit, .. },
        ..
    }, IncomingEvent::Item {
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        payload: NormalizedPayload::FileChangeDiff {
            resource: crate::acp::intent::SafeResourceLocation::Resource { resource },
            operation: crate::acp::intent::FileChangeOperation::Created,
            pair_available: true,
            additions: Some(1),
            deletions: Some(0),
            ..
        },
        ..
    }, IncomingEvent::FilePair { old_text, new_text, .. }] if resource.root_id.as_str() == "root-created"
        && resource.relative_path.as_str() == "cp8-pane-probe.md"
        && old_text.is_empty() && new_text == "probe\n"));
}

#[cfg(windows)]
#[test]
fn modified_file_binds_only_its_final_existing_location() {
    let root = TempRoot::new();
    let mapper = live_mapper(&root);
    let path = root.0.join("edited.md");
    std::fs::write(&path, "before\n").unwrap();
    let absolute = path.to_str().unwrap();
    let mut adapter = active_adapter(&mapper);
    let file = |method, status| {
        json!({"method":method,"params":{
            "threadId":"thread-1","turnId":"turn-1","item":{
                "type":"fileChange","id":"file-modified","status":status,
                "changes":[{"path":absolute,"kind":{"type":"update"},"diff":"opaque"}]
            }
        }})
    };
    let IngressDisposition::Events(started) = adapter
        .ingest(
            &serde_json::to_vec(&file("item/started", "inProgress")).unwrap(),
            &mapper,
        )
        .unwrap()
    else {
        panic!("started notification");
    };
    assert!(started.is_empty());
    std::fs::write(&path, "after\n").unwrap();
    let IngressDisposition::Events(completed) = adapter
        .ingest(
            &serde_json::to_vec(&file("item/completed", "completed")).unwrap(),
            &mapper,
        )
        .unwrap()
    else {
        panic!("completed notification");
    };
    assert!(matches!(completed.as_slice(), [IncomingEvent::Item {
        payload: NormalizedPayload::ToolCall { category: crate::acp::intent::ToolCategory::Edit, .. },
        ..
    }, IncomingEvent::Item {
        lifecycle: ItemLifecycle::Completed,
        payload: NormalizedPayload::FileChangeDiff {
            resource: crate::acp::intent::SafeResourceLocation::Resource { resource },
            operation: crate::acp::intent::FileChangeOperation::Updated,
            ..
        },
        ..
    }] if resource.relative_path.as_str() == "edited.md"));
}

#[cfg(windows)]
#[test]
fn deleted_file_settles_without_an_openable_location() {
    let root = TempRoot::new();
    let mapper = live_mapper(&root);
    let path = root.0.join("deleted.md");
    std::fs::write(&path, "before\n").unwrap();
    let absolute = path
        .to_str()
        .unwrap()
        .strip_prefix("\\\\?\\")
        .unwrap_or(path.to_str().unwrap());
    let mut adapter = active_adapter(&mapper);
    let file = |method, status| {
        json!({"method":method,"params":{
            "threadId":"thread-1","turnId":"turn-1","item":{
                "type":"fileChange","id":"file-deleted","status":status,
                "changes":[{"path":absolute,"kind":{"type":"delete"},"diff":"opaque"}]
            }
        }})
    };
    let IngressDisposition::Events(started) = adapter
        .ingest(
            &serde_json::to_vec(&file("item/started", "inProgress")).unwrap(),
            &mapper,
        )
        .unwrap()
    else {
        panic!("started notification");
    };
    assert!(started.is_empty());
    std::fs::remove_file(&path).unwrap();
    let IngressDisposition::Events(completed) = adapter
        .ingest(
            &serde_json::to_vec(&file("item/completed", "completed")).unwrap(),
            &mapper,
        )
        .unwrap()
    else {
        panic!("completed notification");
    };
    assert!(matches!(
        completed.as_slice(),
        [IncomingEvent::Item {
            payload: NormalizedPayload::ToolCall { category: crate::acp::intent::ToolCategory::Edit, .. },
            ..
        }, IncomingEvent::Item {
            lifecycle: ItemLifecycle::Completed,
            payload: NormalizedPayload::FileChangeDiff {
                resource: crate::acp::intent::SafeResourceLocation::Unavailable {
                    reason: crate::acp::intent::UnavailableResourceReason::Missing,
                    display_resource: Some(display),
                    ..
                },
                operation: crate::acp::intent::FileChangeOperation::Deleted,
                ..
            },
            ..
        }] if display.relative_path.as_str() == "deleted.md"
    ));
}

#[cfg(windows)]
#[test]
fn interrupted_file_change_never_publishes_an_openable_chip() {
    let root = TempRoot::new();
    let mapper = live_mapper(&root);
    let path = root.0.join("interrupted.md");
    std::fs::write(&path, "preexisting\n").unwrap();
    let absolute = path.to_str().unwrap();
    let mut adapter = active_adapter(&mapper);
    let file = |method, status| {
        json!({"method":method,"params":{
            "threadId":"thread-1","turnId":"turn-1","item":{
                "type":"fileChange","id":"file-interrupted","status":status,
                "changes":[{"path":absolute,"kind":{"type":"add"},"diff":"opaque"}]
            }
        }})
    };
    let IngressDisposition::Events(started) = adapter
        .ingest(
            &serde_json::to_vec(&file("item/started", "inProgress")).unwrap(),
            &mapper,
        )
        .unwrap()
    else {
        panic!("started notification");
    };
    assert!(started.is_empty());
    let completed_turn = json!({"method":"turn/completed","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"interrupted"}
    }});
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&completed_turn).unwrap(), &mapper)
        .unwrap()
    else {
        panic!("interrupted turn");
    };
    assert!(matches!(
        events.as_slice(),
        [IncomingEvent::Turn {
            state: TurnState::Interrupted,
            ..
        }]
    ));
    assert_eq!(
        adapter
            .ingest(
                &serde_json::to_vec(&file("item/completed", "completed")).unwrap(),
                &mapper
            )
            .unwrap(),
        IngressDisposition::IgnoredLate
    );
}
