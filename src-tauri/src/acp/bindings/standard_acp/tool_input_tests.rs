use serde_json::{Value, json};

use super::{
    profile::profile_for,
    translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator},
};
use crate::acp::intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason};

struct NoResources;

impl ResourceMapper for NoResources {
    fn map_absolute(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

fn translator(harness: &str) -> StandardAcpTranslator {
    let mut translator =
        StandardAcpTranslator::new(profile_for(harness).unwrap(), "session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    translator
}

fn update(translator: &mut StandardAcpTranslator, update: Value) -> Vec<AdapterEvent> {
    translator
        .translate_notification_value(
            json!({
                "method": "session/update", "params": {"sessionId": "session-1", "update": update}
            }),
            &NoResources,
        )
        .unwrap()
}

fn input(events: &[AdapterEvent]) -> Option<&str> {
    events.iter().find_map(|event| match event {
        AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { input_summary, .. },
            ..
        } => input_summary.as_ref().map(|summary| summary.as_str()),
        _ => None,
    })
}

#[test]
fn only_curated_task_tools_carry_lifecycle_wording_through_completion() {
    for (harness, expected) in [
        ("claude-code", Some(true)),
        ("opencode", None),
        ("gemini-cli", None),
        ("cursor-agent", None),
    ] {
        let mut translator = translator(harness);
        for update_value in [
            json!({"sessionUpdate":"tool_call","toolCallId":"task-1","kind":"other",
                "title":"Review accessibility","status":"in_progress","_meta":{"toolName":"Task"}}),
            json!({"sessionUpdate":"tool_call_update","toolCallId":"task-1","status":"completed"}),
        ] {
            let events = update(&mut translator, update_value);
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, AdapterEvent::Item {
                payload: NormalizedPayload::ToolCall { label, execution, .. }, ..
            } if label.as_str() == "Review accessibility" && execution.task_lifecycle == expected)),
                "{harness}"
            );
        }
    }
}

#[test]
fn task_permission_relabel_and_settlement_preserve_only_curated_lifecycle_metadata() {
    for (harness, expected) in [("claude-code", Some(true)), ("opencode", None)] {
        for cancelled in [false, true] {
            let mut translator = translator(harness);
            update(
                &mut translator,
                json!({"sessionUpdate":"tool_call","toolCallId":"task-1",
                "kind":"other","status":"pending","_meta":{"toolName":"Task"}}),
            );
            let events = translator
                .translate_permission_request_value(json!({
                    "jsonrpc":"2.0","id":7,"method":"session/request_permission",
                    "params":{"sessionId":"session-1","toolCall":{"toolCallId":"task-1",
                        "title":"Review accessibility"},"options":[
                        {"optionId":"allow-once","name":"Allow","kind":"allow_once"},
                        {"optionId":"reject-once","name":"Reject","kind":"reject_once"}]}
                }))
                .unwrap();
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, AdapterEvent::Item {
                external_key, tool_state: Some(crate::acp::lifecycle::ToolState::WaitingForApproval),
                payload: NormalizedPayload::ToolCall { category: crate::acp::intent::ToolCategory::Subagent, label, execution, .. }, ..
            } if external_key.as_str() == "task-1" && label.as_str() == "Review accessibility" && execution.task_lifecycle == expected))
            );
            let settled = if cancelled {
                translator.permission_cancel_committed("7").unwrap()
            } else {
                translator.permission_decline_committed("7").unwrap()
            };
            let state = if cancelled {
                crate::acp::lifecycle::ToolState::Cancelled
            } else {
                crate::acp::lifecycle::ToolState::Declined
            };
            assert!(matches!(settled, AdapterEvent::Item {
                external_key, tool_state: Some(actual),
                payload: NormalizedPayload::ToolCall { category: crate::acp::intent::ToolCategory::Subagent, label, execution, .. }, ..
            } if external_key.as_str() == "task-1" && actual == state && label.as_str() == "Review accessibility" && execution.task_lifecycle == expected));
        }
    }
}

#[test]
fn claude_execute_input_is_sanitized_and_retained_without_permission() {
    let events = update(
        &mut translator("claude-code"),
        json!({
            "sessionUpdate": "tool_call", "toolCallId": "tool-1", "kind": "execute", "status": "in_progress",
            "title": "Run command", "rawInput": {"command": "echo\n--token 'private value'"}
        }),
    );
    assert_eq!(input(&events), Some("echo --token [redacted]"));
}

#[test]
fn a_same_state_late_command_update_is_emitted_and_survives_completion() {
    let mut translator = translator("claude-code");
    let started = update(
        &mut translator,
        json!({
            "sessionUpdate": "tool_call", "toolCallId": "tool-1", "kind": "execute", "status": "in_progress"
        }),
    );
    assert_eq!(input(&started), None);
    let late = update(
        &mut translator,
        json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "rawInput": {"command": "npm run lint"}
        }),
    );
    assert_eq!(input(&late), Some("npm run lint"));
    let completed = update(
        &mut translator,
        json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "status": "completed"
        }),
    );
    assert_eq!(input(&completed), Some("npm run lint"));
    let settled = update(
        &mut translator,
        json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "rawInput": {"command": "npm run build"}
        }),
    );
    assert!(matches!(settled.as_slice(), [AdapterEvent::IgnoredLate]));
    for _ in 0..2 {
        let cleared = update(
            &mut translator,
            json!({
                "sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "rawInput": {"command": null}
            }),
        );
        assert!(matches!(cleared.as_slice(), [AdapterEvent::IgnoredLate]));
    }
}

#[test]
fn missing_malformed_hidden_or_oversized_commands_do_not_invent_input() {
    for raw in [
        Value::Null,
        json!("npm run lint"),
        json!({}),
        json!({"command": []}),
        json!({"command": " \t "}),
        json!({"command": "cat C:/private/key.txt"}),
        json!({"command": "echo\0bad"}),
        json!({"command": "x".repeat(65_537)}),
    ] {
        let events = update(
            &mut translator("claude-code"),
            json!({
                "sessionUpdate": "tool_call", "toolCallId": "tool-1", "kind": "execute", "status": "in_progress",
                "title": "npm run lint", "rawInput": raw
            }),
        );
        assert_eq!(input(&events), None);
    }
}

#[test]
fn an_explicit_invalid_command_field_clears_stale_input_but_an_absent_field_retains_it() {
    for command in [
        json!("cat C:/private/key.txt"),
        json!(" \t "),
        json!({}),
        Value::Null,
    ] {
        let mut translator = translator("claude-code");
        update(
            &mut translator,
            json!({
                "sessionUpdate": "tool_call", "toolCallId": "tool-1", "kind": "execute", "status": "pending",
                "rawInput": {"command": "npm run lint"}
            }),
        );
        let retained = update(
            &mut translator,
            json!({
                "sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "status": "in_progress"
            }),
        );
        assert_eq!(input(&retained), Some("npm run lint"));
        let cleared = update(
            &mut translator,
            json!({
                "sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "rawInput": {"command": command}
            }),
        );
        assert!(cleared.iter().any(|event| matches!(
            event,
            AdapterEvent::Item {
                payload: NormalizedPayload::ToolCall {
                    input_summary: None,
                    ..
                },
                ..
            }
        )));
        let completed = update(
            &mut translator,
            json!({
                "sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "status": "completed"
            }),
        );
        assert_eq!(input(&completed), None);
    }
}

#[test]
fn long_multibyte_command_input_is_ellipsized_within_512_bytes() {
    let events = update(
        &mut translator("claude-code"),
        json!({
            "sessionUpdate": "tool_call", "toolCallId": "tool-1", "kind": "execute", "status": "in_progress",
            "rawInput": {"command": format!("echo {}", "\u{1f680}".repeat(1_000))}
        }),
    );
    let command = input(&events).unwrap();
    assert!(command.len() <= 512);
    assert_eq!(command, format!("echo {}\u{2026}", "\u{1f680}".repeat(126)));
}

#[test]
fn non_execute_claude_and_other_standard_profiles_leave_command_input_absent() {
    for (harness, kind) in [
        ("claude-code", "edit"),
        ("opencode", "execute"),
        ("gemini-cli", "execute"),
        ("cursor-agent", "execute"),
    ] {
        let events = update(
            &mut translator(harness),
            json!({
                "sessionUpdate": "tool_call", "toolCallId": "tool-1", "kind": kind, "status": "in_progress",
                "rawInput": {"command": "npm run lint"}
            }),
        );
        assert_eq!(input(&events), None, "{harness} {kind}");
    }
}
