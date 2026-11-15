use std::cell::Cell;

use serde_json::{Value, json};

use super::profile::{StandardAcpProfile, profile_for};
use super::translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator};
use sage_acp_protocol::intent::{
    FileChangeOperation, NormalizedPayload, SafeResourceLocation, SafeResourceRef,
    UnavailableResourceReason,
};

const PARKED_DIFFS: usize = 16;

struct ForeignResources;

struct NewFileResources;

struct DeletedFileResources;

impl ResourceMapper for DeletedFileResources {
    fn map_absolute(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Missing)
    }

    fn map_missing_display(&self, _absolute_path: &str) -> Option<SafeResourceRef> {
        SafeResourceRef::new(
            sage_acp_protocol::contract::SageRootId::parse("fixture-root").unwrap(),
            "src/deleted.ts",
        )
        .ok()
    }
}

impl ResourceMapper for NewFileResources {
    fn map_absolute(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        SafeResourceRef::new(
            sage_acp_protocol::contract::SageRootId::parse("fixture-root").expect("root"),
            "src/new.ts",
        )
        .map_err(|_| UnavailableResourceReason::Unsupported)
    }
}

impl ResourceMapper for ForeignResources {
    fn map_absolute(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

struct RedirectingResources {
    seen: Cell<usize>,
}

impl ResourceMapper for RedirectingResources {
    fn map_absolute(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        let seen = self.seen.get();
        self.seen.set(seen.saturating_add(1));
        if seen == 0 {
            Err(UnavailableResourceReason::Missing)
        } else {
            Err(UnavailableResourceReason::Foreign)
        }
    }
}

fn claude() -> &'static StandardAcpProfile {
    profile_for("claude-code").expect("claude profile")
}

fn opened() -> StandardAcpTranslator {
    let mut translator = StandardAcpTranslator::new(claude(), "session-1").expect("translator");
    translator.begin_prompt("7").expect("prompt opens");
    translator
}

fn notify(
    translator: &mut StandardAcpTranslator,
    resources: &impl ResourceMapper,
    update: Value,
) -> Vec<AdapterEvent> {
    let frame = json!({
        "method": "session/update",
        "params": { "sessionId": "session-1", "update": update },
    });
    translator
        .translate_notification_value(frame, resources)
        .expect("update translates")
}

fn diff_entry(path: &str) -> Value {
    json!({ "type": "diff", "path": path, "oldText": "before", "newText": "after" })
}

fn park_tool_diffs(
    translator: &mut StandardAcpTranslator,
    tool: &str,
    request: &str,
    diffs: usize,
) {
    notify(
        translator,
        &ForeignResources,
        json!({ "sessionUpdate": "tool_call", "toolCallId": tool, "status": "in_progress" }),
    );
    let content: Vec<Value> = (0..diffs)
        .map(|index| diff_entry(&format!("/fixture/{tool}/file-{index}.rs")))
        .collect();
    notify(
        translator,
        &ForeignResources,
        json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": tool,
            "status": "in_progress",
            "content": content,
        }),
    );
    let permission = json!({
        "jsonrpc": "2.0",
        "id": request,
        "method": "session/request_permission",
        "params": {
            "sessionId": "session-1",
            "toolCall": { "toolCallId": tool },
            "options": [{ "optionId": "allow-once", "name": "Yes", "kind": "allow_once" }],
        },
    });
    translator
        .translate_permission_request_value(permission)
        .expect("permission request translates");
    translator
        .permission_decline_committed(request)
        .expect("decline commits");
}

fn prompt_completed(translator: &mut StandardAcpTranslator) -> Vec<AdapterEvent> {
    translator
        .translate_prompt_result_value(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "result": { "stopReason": "end_turn" },
        }))
        .expect("prompt result translates")
}

fn delivered_diffs(events: &[AdapterEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                AdapterEvent::Item {
                    payload: NormalizedPayload::FileChangeDiff { .. },
                    ..
                }
            )
        })
        .count()
}

fn overflow_summary(events: &[AdapterEvent]) -> Option<String> {
    events.iter().find_map(|event| match event {
        AdapterEvent::Item {
            payload:
                NormalizedPayload::Unsupported {
                    external_kind,
                    summary,
                },
            ..
        } if external_kind.as_str() == "fileChangeOverflow" => Some(summary.as_str().to_owned()),
        _ => None,
    })
}

fn emitted_locations(events: &[AdapterEvent], key: &str) -> Vec<SafeResourceLocation> {
    events
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Item {
                external_key,
                payload: NormalizedPayload::FileChangeDiff { resource, .. },
                ..
            } if external_key.as_str() == key => Some(resource.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn typed_created_diff_names_the_new_file_in_the_tool_card() {
    let mut translator = opened();
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/features/chat/acp/createdFileStandardAcp.fixture.json"
    )))
    .expect("fixture");
    let events = notify(&mut translator, &NewFileResources, fixture["raw"].clone());
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { label, .. },
            ..
        } if label.as_str() == fixture["expected"]["label"].as_str().unwrap()
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::FileChangeDiff {
                operation: FileChangeOperation::Created,
                additions: Some(2),
                deletions: Some(0),
                ..
            },
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::FilePair { old_text, new_text, .. }
            if old_text.is_empty() && new_text == "first\nsecond\n"
    )));
}

#[test]
fn a_completed_missing_diff_with_empty_new_text_is_a_named_nonopenable_deletion() {
    let mut translator = opened();
    let events = notify(
        &mut translator,
        &DeletedFileResources,
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "delete-file",
            "status": "completed",
            "content": [{"type":"diff","path":"C:\\fixture\\src\\deleted.ts","oldText":"before\n","newText":""}]
        }),
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::FileChangeDiff {
                resource: SafeResourceLocation::Unavailable {
                    reason: UnavailableResourceReason::Missing,
                    display_resource: Some(display),
                    ..
                },
                operation: FileChangeOperation::Deleted,
                pair_available: false,
                ..
            },
            ..
        } if display.relative_path.as_str() == "src/deleted.ts"
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AdapterEvent::FilePair { .. }))
    );
}

#[test]
fn write_location_names_the_new_file_without_a_typed_diff() {
    let mut translator = opened();
    let events = notify(
        &mut translator,
        &NewFileResources,
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "write-location",
            "kind": "write",
            "title": "Write file",
            "status": "completed",
            "locations": [{"path": "C:\\fixture\\src\\new.ts"}]
        }),
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { category: sage_acp_protocol::intent::ToolCategory::Edit, label, .. },
            ..
        } if label.as_str() == "Created src/new.ts"
    )));
}

#[test]
fn write_without_a_file_stays_other_activity() {
    let mut translator = opened();
    let events = notify(
        &mut translator,
        &NewFileResources,
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "write-generic",
            "kind": "write",
            "title": "Write generic content",
            "status": "completed"
        }),
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { category: sage_acp_protocol::intent::ToolCategory::Other, label, .. },
            ..
        } if label.as_str() == "Write generic content"
    )));
}

#[test]
fn parked_diffs_beyond_the_frame_budget_are_accounted_for_rather_than_dropped() {
    let mut translator = opened();
    park_tool_diffs(&mut translator, "tool-a", "101", 8);
    park_tool_diffs(&mut translator, "tool-b", "102", 8);
    let events = prompt_completed(&mut translator);
    let delivered = delivered_diffs(&events);
    let summary = overflow_summary(&events).expect("an explicit overflow item is emitted");
    assert_eq!(
        summary,
        "2 file change records could not be delivered in this turn"
    );
    assert_eq!(delivered, 14);
    assert_eq!(delivered + 2, PARKED_DIFFS);
}

#[test]
fn a_drain_that_fits_the_budget_never_claims_an_overflow() {
    let mut translator = opened();
    park_tool_diffs(&mut translator, "tool-a", "101", 7);
    park_tool_diffs(&mut translator, "tool-b", "102", 7);
    let events = prompt_completed(&mut translator);
    assert_eq!(delivered_diffs(&events), 14);
    assert_eq!(overflow_summary(&events), None);
}

#[test]
fn a_junction_redirect_between_live_updates_never_restates_the_bound_identity() {
    let mut translator = opened();
    let resources = RedirectingResources { seen: Cell::new(0) };
    notify(
        &mut translator,
        &resources,
        json!({ "sessionUpdate": "tool_call", "toolCallId": "tool-j", "status": "in_progress" }),
    );
    let first = notify(
        &mut translator,
        &resources,
        json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "tool-j",
            "status": "in_progress",
            "content": [diff_entry("/fixture/junction/target.rs")],
        }),
    );
    let second = notify(
        &mut translator,
        &resources,
        json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "tool-j",
            "status": "in_progress",
            "content": [diff_entry("/fixture/junction/target.rs")],
        }),
    );
    let key = "tool-j:diff:0";
    let mut seen = emitted_locations(&first, key);
    seen.extend(emitted_locations(&second, key));
    assert!(seen.len() >= 2, "both frames emit the bound path");
    let frozen = seen.first().expect("a first identity").clone();
    assert_eq!(
        frozen,
        SafeResourceLocation::unavailable(UnavailableResourceReason::Missing)
    );
    for location in &seen {
        assert_eq!(location, &frozen, "the bound identity never changes");
    }
}
