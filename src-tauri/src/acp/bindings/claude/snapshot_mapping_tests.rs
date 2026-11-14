use serde_json::json;

use super::{AdapterEvent, ClaudeTranslator, ResourceMapper, TranslationError, claude_profile};
use crate::{
    acp::{
        contract::SageRootId,
        intent::{
            FileChangeOperation, NormalizedPayload, SafeResourceLocation, SafeResourceRef,
            UnavailableResourceReason,
        },
        lifecycle::ItemLifecycle,
    },
    acp_protocol::file_diff::{DIFF_SIDE_BYTES_MAX, admit_pair, pair_digest},
};

struct FixtureResources;

impl ResourceMapper for FixtureResources {
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        if absolute_path == r"C:\fixture\root\src\lib.rs" {
            SafeResourceRef::new(
                SageRootId::parse("root-fixture")
                    .map_err(|_| UnavailableResourceReason::Unsupported)?,
                "src/lib.rs",
            )
            .map_err(|_| UnavailableResourceReason::Unsupported)
        } else {
            Err(UnavailableResourceReason::Foreign)
        }
    }
}

fn encoded(value: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

struct MissingResources;

impl ResourceMapper for MissingResources {
    fn map_absolute(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Missing)
    }
}

fn intended_change_resource(content: serde_json::Value) -> SafeResourceLocation {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    let start = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "tool-diff",
                "kind": "edit",
                "status": "pending",
            }
        }
    });
    translator
        .translate_notification(&encoded(&start), &MissingResources)
        .unwrap();
    let frame = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "tool-diff",
                "kind": "edit",
                "status": "completed",
                "content": content,
            }
        }
    });
    let events = translator
        .translate_notification(&encoded(&frame), &MissingResources)
        .unwrap();
    events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload: NormalizedPayload::FileChangeDiff { resource, .. },
                ..
            } => Some(resource.clone()),
            _ => None,
        })
        .expect("a file change carries a resource location")
}

#[test]
fn claude_creation_of_a_path_the_harness_has_not_written_stays_unavailable() {
    let resource = intended_change_resource(json!([{
        "type": "diff",
        "path": r"C:\fixture\root\src\created.rs",
        "oldText": null,
        "newText": "created\n",
    }]));
    assert_eq!(
        resource,
        SafeResourceLocation::unavailable(UnavailableResourceReason::Missing)
    );
}

#[test]
fn claude_update_of_a_missing_path_stays_unavailable() {
    let resource = intended_change_resource(json!([{
        "type": "diff",
        "path": r"C:\fixture\root\src\created.rs",
        "oldText": "old\n",
        "newText": "new\n",
    }]));
    assert_eq!(
        resource,
        SafeResourceLocation::unavailable(UnavailableResourceReason::Missing)
    );
}

fn start_tool(translator: &mut ClaudeTranslator) {
    let frame = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "tool-diff",
                "kind": "edit",
                "status": "pending",
            }
        }
    });
    translator
        .translate_notification(&encoded(&frame), &FixtureResources)
        .unwrap();
}

fn complete_with(
    translator: &mut ClaudeTranslator,
    content: serde_json::Value,
) -> Vec<AdapterEvent> {
    let frame = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "tool-diff",
                "kind": "edit",
                "status": "completed",
                "content": content,
            }
        }
    });
    translator
        .translate_notification(&encoded(&frame), &FixtureResources)
        .unwrap()
}

#[test]
fn claude_multi_entry_pair_maps_to_shared_typed_shape() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    start_tool(&mut translator);
    let events = complete_with(
        &mut translator,
        json!([
            {"type":"diff","path":r"C:\fixture\root\src\lib.rs","oldText":"old\n","newText":"new\n"},
            {"type":"diff","path":r"C:\fixture\root\src\lib.rs","oldText":"old\n","newText":"other\n"},
        ]),
    );
    let pairs: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::FilePair {
                old_text, new_text, ..
            } => Some((old_text.clone(), new_text.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(pairs.len(), 2);
    for (old, new) in &pairs {
        let admitted = admit_pair(old, new);
        assert!(matches!(
            admitted,
            crate::acp_protocol::file_diff::PairAdmission::Admitted { .. }
        ));
    }
    let items: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Item {
                payload:
                    NormalizedPayload::FileChangeDiff {
                        content_digest,
                        old_bytes,
                        new_bytes,
                        pair_available: true,
                        ..
                    },
                ..
            } => Some((content_digest.clone(), *old_bytes, *new_bytes)),
            _ => None,
        })
        .collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].0, pair_digest(b"old\n", b"new\n"));
    assert_eq!(items[1].0, pair_digest(b"old\n", b"other\n"));
    assert!(!format!("{events:?}").contains(r"C:\fixture"));
}

#[test]
fn claude_duplicate_late_and_cancel_are_typed_without_text() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    start_tool(&mut translator);
    let first = complete_with(
        &mut translator,
        json!([{"type":"diff","path":r"C:\fixture\root\src\lib.rs","oldText":"a","newText":"b"}]),
    );
    assert!(
        first
            .iter()
            .any(|event| matches!(event, AdapterEvent::FilePair { .. }))
    );
    let duplicate = complete_with(&mut translator, json!([]));
    assert!(
        duplicate
            .iter()
            .any(|event| matches!(event, AdapterEvent::IgnoredDuplicate))
    );
    let late = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "unknown-tool",
                "kind": "edit",
                "status": "completed",
                "content": [],
            }
        }
    });
    assert!(matches!(
        translator.translate_notification(&encoded(&late), &FixtureResources),
        Err(TranslationError::OrphanToolUpdate)
    ));
}

#[test]
fn claude_oversize_and_foreign_map_to_summary_only_without_partial_text() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    start_tool(&mut translator);
    let big = "x".repeat(DIFF_SIDE_BYTES_MAX + 1);
    let events = complete_with(
        &mut translator,
        json!([{"type":"diff","path":r"C:\fixture\root\src\lib.rs","oldText":big,"newText":"ok"}]),
    );
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, AdapterEvent::FilePair { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::FileChangeDiff {
                pair_available: false,
                truncated: true,
                ..
            },
            lifecycle: ItemLifecycle::Completed,
            ..
        }
    )));
    assert!(!format!("{events:?}").contains(&big[..64]));

    let mut foreign = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    foreign.begin_prompt("prompt-1").unwrap();
    let start = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "tool-foreign",
                "kind": "edit",
                "status": "pending",
            }
        }
    });
    foreign
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();
    let frame = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "tool-foreign",
                "kind": "edit",
                "status": "completed",
                "content": [{"type":"diff","path":r"C:\foreign\x.rs","oldText":"a","newText":"b"}],
            }
        }
    });
    let mapped = foreign
        .translate_notification(&encoded(&frame), &FixtureResources)
        .unwrap();
    assert!(
        mapped
            .iter()
            .all(|event| !matches!(event, AdapterEvent::FilePair { .. }))
    );
}

fn creation_events(content: serde_json::Value) -> Vec<AdapterEvent> {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    start_tool(&mut translator);
    complete_with(&mut translator, content)
}

fn admitted_creation(events: &[AdapterEvent]) -> (String, String, u64) {
    let pair = events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::FilePair {
                old_text, new_text, ..
            } => Some((old_text.clone(), new_text.clone())),
            _ => None,
        })
        .expect("a created file still reports one admitted pair");
    let old_bytes = events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload: NormalizedPayload::FileChangeDiff { old_bytes, .. },
                ..
            } => Some(*old_bytes),
            _ => None,
        })
        .expect("a created file still reports typed diff metadata");
    (pair.0, pair.1, old_bytes)
}

#[test]
fn claude_file_creation_with_null_old_text_admits_an_empty_original() {
    let events = creation_events(json!([{
        "type": "diff",
        "path": r"C:\fixture\root\src\lib.rs",
        "oldText": null,
        "newText": "created\n",
    }]));
    let (old_text, new_text, old_bytes) = admitted_creation(&events);
    assert_eq!(old_text, "");
    assert_eq!(new_text, "created\n");
    assert_eq!(old_bytes, 0);
}

fn change_shape(events: &[AdapterEvent]) -> (FileChangeOperation, Option<u64>, Option<u64>) {
    events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload:
                    NormalizedPayload::FileChangeDiff {
                        operation,
                        additions,
                        deletions,
                        ..
                    },
                ..
            } => Some((*operation, *additions, *deletions)),
            _ => None,
        })
        .expect("a file change reports typed diff metadata")
}

#[test]
fn claude_null_old_text_marks_the_change_created_with_one_addition_and_no_deletion() {
    let events = creation_events(json!([{
        "type": "diff",
        "path": r"C:\fixture\root\src\lib.rs",
        "oldText": null,
        "newText": "created\n",
    }]));
    assert_eq!(
        change_shape(&events),
        (FileChangeOperation::Created, Some(1), Some(0))
    );
}

#[test]
fn claude_absent_old_text_marks_the_change_created() {
    let events = creation_events(json!([{
        "type": "diff",
        "path": r"C:\fixture\root\src\lib.rs",
        "newText": "created\n",
    }]));
    assert_eq!(change_shape(&events).0, FileChangeOperation::Created);
}

#[test]
fn claude_explicitly_empty_old_text_stays_an_update_of_an_existing_empty_file() {
    let events = creation_events(json!([{
        "type": "diff",
        "path": r"C:\fixture\root\src\lib.rs",
        "oldText": "",
        "newText": "created\n",
    }]));
    assert_eq!(change_shape(&events).0, FileChangeOperation::Updated);
}

#[test]
fn claude_file_creation_without_old_text_admits_an_empty_original() {
    let events = creation_events(json!([{
        "type": "diff",
        "path": r"C:\fixture\root\src\lib.rs",
        "newText": "created\n",
    }]));
    let (old_text, new_text, old_bytes) = admitted_creation(&events);
    assert_eq!(old_text, "");
    assert_eq!(new_text, "created\n");
    assert_eq!(old_bytes, 0);
}
