use serde_json::json;

use super::{AdapterEvent, ClaudeTranslator, ResourceMapper, TranslationError};
use crate::acp::{
    contract::SageRootId,
    file_diff::{DIFF_SIDE_BYTES_MAX, admit_pair, pair_digest},
    intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason},
    lifecycle::ItemLifecycle,
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
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
            crate::acp::file_diff::PairAdmission::Admitted { .. }
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
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
    // Duplicate tool update with identical state and no new content is ignored.
    let duplicate = complete_with(&mut translator, json!([]));
    assert!(
        duplicate
            .iter()
            .any(|event| matches!(event, AdapterEvent::IgnoredDuplicate))
    );
    // Late update for an unknown tool is a typed error, never invented text.
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
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

    let mut foreign = ClaudeTranslator::new("claude-session-1").unwrap();
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
