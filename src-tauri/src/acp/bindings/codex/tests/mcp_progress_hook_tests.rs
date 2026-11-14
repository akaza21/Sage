use super::*;
use crate::acp::{
    intent::{
        ToolCategory,
        session_event::{SessionActivity, SessionActivityState, SessionScopedEvent},
    },
    lifecycle::{ItemLifecycle, ToolState},
};

use super::state::ExternalKind;

fn refusal(result: Result<IngressDisposition, AdapterError>) -> Option<IngressError> {
    match result {
        Err(AdapterError::Ingress(error)) => Some(error),
        _ => None,
    }
}

fn frame(value: Value) -> Vec<u8> {
    serde_json::to_vec(&value).unwrap()
}

fn running_adapter() -> CodexAdapter {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-mcp-progress");
    adapter
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
            }})),
            &Mapper,
        )
        .unwrap();
    adapter
}

fn mcp_item(method: &str, status: &str, result: Value) -> Vec<u8> {
    frame(json!({"method":method,"params":{
        "threadId":"thread-1","turnId":"turn-1",
        "item":{"type":"mcpToolCall","id":"mcp-1","server":"docs","tool":"search",
            "status":status,"arguments":{"query":"hooks"},"result":result,"error":null}
    }}))
}

fn progress(item: &str, message: &str) -> Vec<u8> {
    frame(json!({"method":"item/mcpToolCall/progress","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":item,"message":message
    }}))
}

fn item_update(
    disposition: IngressDisposition,
) -> (String, ItemLifecycle, Option<ToolState>, Option<String>) {
    let IngressDisposition::Events(events) = disposition else {
        panic!("expected an item update, got {disposition:?}")
    };
    let [
        IncomingEvent::Item {
            id,
            lifecycle,
            tool_state,
            payload:
                NormalizedPayload::ToolCall {
                    category: ToolCategory::Mcp,
                    execution,
                    ..
                },
            ..
        },
    ] = events.as_slice()
    else {
        panic!("expected one MCP item, got {events:?}")
    };
    (
        id.external_id.as_str().to_owned(),
        *lifecycle,
        *tool_state,
        execution
            .progress
            .as_ref()
            .map(|line| line.as_str().to_owned()),
    )
}

#[test]
fn mcp_progress_restates_the_running_call_with_only_its_latest_line() {
    let mut adapter = running_adapter();
    let started = adapter
        .ingest(
            &mcp_item("item/started", "inProgress", Value::Null),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        item_update(started),
        (
            "mcp-1".into(),
            ItemLifecycle::Started,
            Some(ToolState::InProgress),
            None
        )
    );

    for line in [
        "Indexed 1 of 3 pages",
        "Indexed 2 of 3 pages",
        "Indexed 3 of 3 pages",
    ] {
        let update = adapter.ingest(&progress("mcp-1", line), &Mapper).unwrap();
        assert_eq!(
            item_update(update),
            (
                "mcp-1".into(),
                ItemLifecycle::Streaming,
                Some(ToolState::InProgress),
                Some(line.into())
            ),
            "each report replaces the same item's status line"
        );
    }
    assert_eq!(
        adapter
            .ingest(&progress("mcp-1", "Indexed 3 of 3 pages"), &Mapper)
            .unwrap(),
        IngressDisposition::IgnoredLate,
        "an unchanged line has nothing to restate"
    );

    let result = json!({"content":[{"type":"text","text":"3 matches"}]});
    let completed = adapter
        .ingest(&mcp_item("item/completed", "completed", result), &Mapper)
        .unwrap();
    assert_eq!(
        item_update(completed),
        (
            "mcp-1".into(),
            ItemLifecycle::Completed,
            Some(ToolState::Completed),
            None
        ),
        "the terminal item carries no running status line"
    );
}

#[test]
fn mcp_progress_after_completion_or_for_an_unknown_call_is_ignored() {
    let mut adapter = running_adapter();
    adapter
        .ingest(
            &mcp_item("item/started", "inProgress", Value::Null),
            &Mapper,
        )
        .unwrap();
    adapter
        .ingest(
            &mcp_item("item/completed", "completed", json!({"content":[]})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        adapter
            .ingest(&progress("mcp-1", "late line"), &Mapper)
            .unwrap(),
        IngressDisposition::IgnoredLate
    );
    assert_eq!(
        adapter
            .ingest(&progress("mcp-unknown", "orphan line"), &Mapper)
            .unwrap(),
        IngressDisposition::IgnoredLate
    );
}

#[test]
fn mcp_progress_of_a_finished_turn_is_ignored_and_a_foreign_or_malformed_one_is_refused() {
    let mut adapter = running_adapter();
    adapter
        .ingest(
            &mcp_item("item/started", "inProgress", Value::Null),
            &Mapper,
        )
        .unwrap();
    let foreign = frame(json!({"method":"item/mcpToolCall/progress","params":{
        "threadId":"thread-1","turnId":"turn-other","itemId":"mcp-1","message":"x"
    }}));
    assert_eq!(
        refusal(adapter.ingest(&foreign, &Mapper)),
        Some(IngressError::Identity)
    );
    let missing_message = frame(json!({"method":"item/mcpToolCall/progress","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"mcp-1"
    }}));
    assert_eq!(
        refusal(adapter.ingest(&missing_message, &Mapper)),
        Some(IngressError::Malformed)
    );

    adapter
        .ingest(
            &frame(json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"interrupted"}
            }})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        adapter
            .ingest(&progress("mcp-1", "after the turn"), &Mapper)
            .unwrap(),
        IngressDisposition::IgnoredLate
    );
}

#[test]
fn mcp_progress_line_is_redacted_and_byte_capped() {
    let mut adapter = running_adapter();
    adapter
        .ingest(
            &mcp_item("item/started", "inProgress", Value::Null),
            &Mapper,
        )
        .unwrap();
    let update = adapter
        .ingest(&progress("mcp-1", "using sk-live-secret\nnow"), &Mapper)
        .unwrap();
    assert_eq!(
        item_update(update).3.as_deref(),
        Some("using [redacted] now")
    );

    let update = adapter
        .ingest(&progress("mcp-1", &"p".repeat(64 * 1024)), &Mapper)
        .unwrap();
    let line = item_update(update).3.unwrap();
    assert!(line.len() <= crate::acp::intent::TOOL_PROGRESS_BYTES);
    assert!(line.ends_with('\u{2026}'));
}

const HOOK_SOURCE: &str = "C:\\Users\\dev\\.codex\\hooks.json";

fn hook(method: &str, turn: Value, status: &str, entries: Value) -> Vec<u8> {
    frame(json!({"method":method,"params":{
        "threadId":"thread-1","turnId":turn,
        "run":{
            "id":format!("pre-tool-use:1:{HOOK_SOURCE}"),
            "eventName":"preToolUse","handlerType":"command","executionMode":"sync",
            "scope":"turn","sourcePath":HOOK_SOURCE,"source":"project","displayOrder":1,
            "status":status,"statusMessage":"Checking shell policy",
            "startedAt":1_700_000_000,
            "completedAt":if status == "running" { Value::Null } else { json!(1_700_000_001) },
            "durationMs":if status == "running" { Value::Null } else { json!(42) },
            "entries":entries
        }
    }}))
}

fn hook_activity(disposition: Result<IngressDisposition, AdapterError>) -> SessionActivity {
    let Ok(IngressDisposition::Events(events)) = disposition else {
        panic!("expected a hook activity, got {disposition:?}")
    };
    let [IncomingEvent::Session { id, event }] = events.as_slice() else {
        panic!("expected one session event, got {events:?}")
    };
    assert_eq!(id.kind, ExternalKind::Session);
    assert_eq!(id.external_id.as_str(), "thread-1");
    assert_eq!(id.turn_id, None);
    let SessionScopedEvent::Activity(activity) = event else {
        panic!("expected an activity, got {event:?}")
    };
    assert_eq!(activity.kind.as_str(), "hook");
    assert_eq!(activity.label.as_str(), "PreToolUse hook #2 (project)");
    let digest = activity
        .id
        .as_str()
        .strip_prefix("hook-")
        .expect("a hook row id is prefixed");
    assert!(
        digest.len() == 32 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "the row id is a path-free digest: {}",
        activity.id.as_str()
    );
    activity.clone()
}

#[test]
fn a_passing_hook_runs_then_completes_on_one_activity_row() {
    let mut adapter = running_adapter();
    let started = hook_activity(adapter.ingest(
        &hook("hook/started", json!("turn-1"), "running", json!([])),
        &Mapper,
    ));
    assert_eq!(started.state, SessionActivityState::Running);
    assert_eq!(
        started.progress.as_ref().map(|line| line.as_str()),
        Some("Checking shell policy")
    );
    let completed = hook_activity(adapter.ingest(
        &hook(
            "hook/completed",
            json!("turn-1"),
            "completed",
            json!([{"kind":"context","text":"model-only context"}]),
        ),
        &Mapper,
    ));
    assert_eq!(completed.id, started.id);
    assert_eq!(completed.state, SessionActivityState::Completed);
    assert_eq!(completed.progress, None);
}

#[test]
fn a_blocking_hook_completes_with_its_reason_and_a_failed_one_fails() {
    let mut adapter = running_adapter();
    let blocked = hook_activity(adapter.ingest(
        &hook(
            "hook/completed",
            json!("turn-1"),
            "blocked",
            json!([{"kind":"feedback","text":"rm -rf is not allowed"}]),
        ),
        &Mapper,
    ));
    assert_eq!(blocked.state, SessionActivityState::Completed);
    assert_eq!(
        blocked.progress.as_ref().map(|line| line.as_str()),
        Some("Blocked: rm -rf is not allowed")
    );

    let failed = hook_activity(adapter.ingest(
        &hook(
            "hook/completed",
            json!("turn-1"),
            "failed",
            json!([{"kind":"error","text":"hook exited with code 1"}]),
        ),
        &Mapper,
    ));
    assert_eq!(failed.state, SessionActivityState::Failed);
    assert_eq!(
        failed.progress.as_ref().map(|line| line.as_str()),
        Some("hook exited with code 1")
    );
}

#[test]
fn a_turnless_hook_is_a_session_activity_without_any_turn() {
    let mut adapter = adapter();
    ready(&mut adapter);
    let started = hook_activity(adapter.ingest(
        &hook("hook/started", Value::Null, "running", json!([])),
        &Mapper,
    ));
    assert_eq!(started.state, SessionActivityState::Running);
    let completed = hook_activity(adapter.ingest(
        &hook("hook/completed", Value::Null, "stopped", json!([])),
        &Mapper,
    ));
    assert_eq!(completed.state, SessionActivityState::Cancelled);
    assert_eq!(
        completed.progress.as_ref().map(|line| line.as_str()),
        Some("Stopped")
    );
}

#[test]
fn a_hook_with_a_foreign_thread_or_an_invalid_run_is_refused() {
    let mut adapter = running_adapter();
    let mut foreign: Value =
        serde_json::from_slice(&hook("hook/started", json!("turn-1"), "running", json!([])))
            .unwrap();
    foreign["params"]["threadId"] = json!("thread-other");
    assert_eq!(
        refusal(adapter.ingest(&frame(foreign), &Mapper)),
        Some(IngressError::Identity)
    );

    for (field, value) in [
        ("eventName", json!(7)),
        ("displayOrder", json!("first")),
        ("statusMessage", json!(7)),
        ("entries", json!(["not an entry"])),
        ("id", json!("")),
    ] {
        let mut invalid: Value =
            serde_json::from_slice(&hook("hook/started", json!("turn-1"), "running", json!([])))
                .unwrap();
        invalid["params"]["run"][field] = value;
        assert_eq!(
            refusal(adapter.ingest(&frame(invalid), &Mapper)),
            Some(IngressError::Malformed),
            "{field} must be validated"
        );
    }
    assert_eq!(
        refusal(adapter.ingest(
            &hook("hook/started", json!("turn-1"), "completed", json!([])),
            &Mapper
        )),
        Some(IngressError::Malformed),
        "a started hook must be running"
    );
    assert_eq!(
        refusal(adapter.ingest(
            &hook("hook/completed", json!("turn-1"), "running", json!([])),
            &Mapper
        )),
        Some(IngressError::Malformed),
        "a completed hook must be terminal"
    );
}

fn turn_completed() -> Vec<u8> {
    frame(json!({"method":"turn/completed","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"interrupted"}
    }}))
}

#[test]
fn a_hook_still_running_when_its_turn_ends_is_settled_as_cancelled() {
    let mut adapter = running_adapter();
    let started = hook_activity(adapter.ingest(
        &hook("hook/started", json!("turn-1"), "running", json!([])),
        &Mapper,
    ));
    let IngressDisposition::Events(events) = adapter.ingest(&turn_completed(), &Mapper).unwrap()
    else {
        panic!("the turn terminal must be projected")
    };
    let [
        IncomingEvent::Session {
            event: SessionScopedEvent::Activity(cancelled),
            ..
        },
        IncomingEvent::Turn { .. },
    ] = events.as_slice()
    else {
        panic!("expected the cancelled hook and the turn, got {events:?}")
    };
    assert_eq!(cancelled.id, started.id);
    assert_eq!(cancelled.state, SessionActivityState::Cancelled);
    assert_eq!(cancelled.progress, None);
}

#[test]
fn a_hook_that_settles_after_its_turn_ended_names_that_previous_turn() {
    let mut adapter = running_adapter();
    let started = hook_activity(adapter.ingest(
        &hook("hook/started", json!("turn-1"), "running", json!([])),
        &Mapper,
    ));
    assert!(!started.in_previous_turn, "a hook of the running Turn");
    adapter.ingest(&turn_completed(), &Mapper).unwrap();
    let late = hook_activity(adapter.ingest(
        &hook("hook/completed", json!("turn-1"), "completed", json!([])),
        &Mapper,
    ));
    assert_eq!(late.id, started.id);
    assert!(
        late.in_previous_turn,
        "the store must not file it under the next Turn"
    );
    let turnless = hook_activity(adapter.ingest(
        &hook("hook/started", Value::Null, "running", json!([])),
        &Mapper,
    ));
    assert!(!turnless.in_previous_turn);
}

#[test]
fn a_completed_or_turnless_hook_is_not_cancelled_when_the_turn_ends() {
    let mut adapter = running_adapter();
    adapter
        .ingest(
            &hook("hook/started", json!("turn-1"), "running", json!([])),
            &Mapper,
        )
        .unwrap();
    adapter
        .ingest(
            &hook("hook/completed", json!("turn-1"), "completed", json!([])),
            &Mapper,
        )
        .unwrap();
    adapter
        .ingest(
            &hook("hook/started", Value::Null, "running", json!([])),
            &Mapper,
        )
        .unwrap();
    let IngressDisposition::Events(events) = adapter.ingest(&turn_completed(), &Mapper).unwrap()
    else {
        panic!("the turn terminal must be projected")
    };
    assert!(
        matches!(events.as_slice(), [IncomingEvent::Turn { .. }]),
        "only hooks of the ending turn are cancelled: {events:?}"
    );
}

#[test]
fn hook_text_is_redacted_before_it_is_shown() {
    let mut adapter = running_adapter();
    let failed = hook_activity(adapter.ingest(
        &hook(
            "hook/completed",
            json!("turn-1"),
            "failed",
            json!([{"kind":"error","text":
                "cannot read /home/someone/.ssh/id_ed25519 using token sk-live-abcdef0123456789"}]),
        ),
        &Mapper,
    ));
    let line = failed.progress.expect("the failure says why");
    let line = line.as_str();
    assert!(line.starts_with("cannot read"), "{line}");
    assert!(!line.contains("/home/someone"), "{line}");
    assert!(!line.contains("sk-live-abcdef0123456789"), "{line}");
}

#[test]
fn running_mcp_calls_are_cleared_when_the_turn_ends() {
    let mut adapter = running_adapter();
    adapter
        .ingest(
            &mcp_item("item/started", "inProgress", Value::Null),
            &Mapper,
        )
        .unwrap();
    assert_eq!(adapter.running_mcp_tool_count(), 1);
    adapter.ingest(&turn_completed(), &Mapper).unwrap();
    assert_eq!(adapter.running_mcp_tool_count(), 0);
}

#[test]
fn the_oldest_running_mcp_call_is_forgotten_beyond_the_cap() {
    let mut adapter = running_adapter();
    let cap = super::state::RUNNING_MCP_TOOLS_MAX;
    for index in 0..=cap {
        adapter
            .ingest(
                &frame(json!({"method":"item/started","params":{
                    "threadId":"thread-1","turnId":"turn-1",
                    "item":{"type":"mcpToolCall","id":format!("mcp-{index}"),
                        "server":"docs","tool":"search","status":"inProgress",
                        "arguments":{},"result":null,"error":null}
                }})),
                &Mapper,
            )
            .unwrap();
    }
    assert_eq!(adapter.running_mcp_tool_count(), cap);
    assert_eq!(
        adapter
            .ingest(&progress("mcp-0", "late for the oldest"), &Mapper)
            .unwrap(),
        IngressDisposition::IgnoredLate,
        "the evicted oldest call is unknown"
    );
    let newest = format!("mcp-{cap}");
    assert_eq!(
        item_update(
            adapter
                .ingest(&progress(&newest, "still tracked"), &Mapper)
                .unwrap()
        )
        .3,
        Some("still tracked".into())
    );
}

#[test]
fn a_hook_run_from_a_newer_codex_is_still_shown() {
    let mut adapter = running_adapter();
    let mut newer: Value =
        serde_json::from_slice(&hook("hook/started", json!("turn-1"), "running", json!([])))
            .unwrap();
    let run = newer["params"]["run"].as_object_mut().unwrap();
    for field in ["handlerType", "executionMode", "scope", "sourcePath"] {
        run.remove(field);
    }
    run.insert("eventName".into(), json!("preModelCall"));
    run.insert("source".into(), json!("workspacePolicy"));
    run.insert(
        "entries".into(),
        json!([{"kind":"notice","text":"New entry kind"}]),
    );
    let Ok(IngressDisposition::Events(events)) = adapter.ingest(&frame(newer), &Mapper) else {
        panic!("a newer hook run is shown, not refused")
    };
    let [
        IncomingEvent::Session {
            event: SessionScopedEvent::Activity(activity),
            ..
        },
    ] = events.as_slice()
    else {
        panic!("expected one hook activity, got {events:?}")
    };
    assert_eq!(activity.label.as_str(), "Codex hook #2 (unknown)");
}
