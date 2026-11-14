use serde_json::{Value, json};

use super::{
    profile::profile_for,
    translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator},
};
use sage_acp_protocol::intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason};

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
                external_key, tool_state: Some(sage_acp_protocol::lifecycle::ToolState::WaitingForApproval),
                payload: NormalizedPayload::ToolCall { category: sage_acp_protocol::intent::ToolCategory::Subagent, label, execution, .. }, ..
            } if external_key.as_str() == "task-1" && label.as_str() == "Review accessibility" && execution.task_lifecycle == expected))
            );
            let settled = if cancelled {
                translator.permission_cancel_committed("7").unwrap()
            } else {
                translator.permission_decline_committed("7").unwrap()
            };
            let state = if cancelled {
                sage_acp_protocol::lifecycle::ToolState::Cancelled
            } else {
                sage_acp_protocol::lifecycle::ToolState::Declined
            };
            assert!(matches!(settled, AdapterEvent::Item {
                external_key, tool_state: Some(actual),
                payload: NormalizedPayload::ToolCall { category: sage_acp_protocol::intent::ToolCategory::Subagent, label, execution, .. }, ..
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
fn long_multibyte_command_input_is_retained_without_ellipsizing() {
    let events = update(
        &mut translator("claude-code"),
        json!({
            "sessionUpdate": "tool_call", "toolCallId": "tool-1", "kind": "execute", "status": "in_progress",
            "rawInput": {"command": format!("echo {}", "\u{1f680}".repeat(1_000))}
        }),
    );
    let command = input(&events).unwrap();
    assert_eq!(command, format!("echo {}", "\u{1f680}".repeat(1_000)));
}

#[test]
fn command_detail_retains_the_exact_bound_and_redacts_secrets_after_the_old_row_limit() {
    for command in [
        "x".repeat(65_536),
        format!("echo {} --token sk-hidden", "x".repeat(1_000)),
    ] {
        let events = update(
            &mut translator("claude-code"),
            json!({
                "sessionUpdate": "tool_call", "toolCallId": "tool-1", "kind": "execute",
                "rawInput": {"command": command}
            }),
        );
        let expected = command.replace("sk-hidden", "[redacted]");
        assert_eq!(input(&events), Some(expected.as_str()));
    }
}

#[test]
fn claude_command_output_projects_raw_or_console_content_once_and_retains_it_at_settlement() {
    for output_fields in [
        json!({"rawOutput": "ok --token sk-hidden", "content": [{"type":"content","content":{"type":"text","text":"```console\nok --token sk-hidden\n```"}}]}),
        json!({"content": [{"type":"content","content":{"type":"text","text":"```console\nok --token sk-hidden\n```"}}]}),
        json!({"content": [{"type":"text","text":"ok --token sk-hidden"}]}),
        json!({"rawOutput": "ok --token 'private value'", "content": [{"type":"content","content":{"type":"text","text":"```console\nok --token 'private value'\n```"}}]}),
        json!({"content": [{"type":"content","content":{"type":"text","text":"```console\nok --token 'private value'\n```"}}]}),
        json!({"content": [{"type":"text","text":"ok --token 'private value'"}]}),
    ] {
        let mut translator = translator("claude-code");
        let mut frame = json!({"sessionUpdate":"tool_call","toolCallId":"command","kind":"execute","status":"in_progress"});
        frame
            .as_object_mut()
            .unwrap()
            .extend(output_fields.as_object().unwrap().clone());
        let started = update(&mut translator, frame);
        let completed = update(
            &mut translator,
            json!({"sessionUpdate":"tool_call_update","toolCallId":"command","status":"completed"}),
        );
        for events in [started, completed] {
            let outputs: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    AdapterEvent::Item {
                        payload:
                            NormalizedPayload::ToolCall {
                                output_summary: Some(output),
                                ..
                            },
                        ..
                    } => Some(output.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(outputs, ["ok --token [redacted]"]);
        }
    }
}

#[test]
fn retained_command_input_never_admits_a_path_beyond_the_old_row_limit() {
    for suffix in [
        ">/home/alice/private.txt",
        r">>C:\Users\Alice\private.txt",
        "</private.txt",
        "|/home/alice/private.txt",
        ";/home/alice/private.txt",
        "&&/home/alice/private.txt",
        "$(/home/alice/private.txt)",
        "`/home/alice/private.txt`",
    ] {
        let events = update(
            &mut translator("claude-code"),
            json!({"sessionUpdate": "tool_call", "toolCallId": "tool-1", "kind": "execute",
                "rawInput": {"command": format!("echo {}{suffix}", "x".repeat(600))}}),
        );
        assert_eq!(input(&events), None, "{suffix}");
    }
}

#[test]
fn every_standard_profile_carries_an_execute_command_and_no_other_kind_does() {
    for (harness, kind, expected) in [
        ("claude-code", "edit", None),
        ("gemini-cli", "read", None),
        ("claude-code", "execute", Some("npm run lint")),
        ("opencode", "execute", Some("npm run lint")),
        ("gemini-cli", "execute", Some("npm run lint")),
        ("cursor-agent", "execute", Some("npm run lint")),
    ] {
        let events = update(
            &mut translator(harness),
            json!({
                "sessionUpdate": "tool_call", "toolCallId": "tool-1", "kind": kind, "status": "in_progress",
                "rawInput": {"command": "npm run lint"}
            }),
        );
        assert_eq!(input(&events), expected, "{harness} {kind}");
    }
}

fn execution_of(
    events: &[AdapterEvent],
) -> Option<(&str, &sage_acp_protocol::intent::ToolExecution)> {
    events.iter().find_map(|event| match event {
        AdapterEvent::Item {
            payload:
                NormalizedPayload::ToolCall {
                    label, execution, ..
                },
            ..
        } => Some((label.as_str(), execution)),
        _ => None,
    })
}

#[test]
fn an_untitled_command_reads_as_its_command_line_not_a_stand_in() {
    let events = update(
        &mut translator("gemini-cli"),
        json!({"sessionUpdate":"tool_call","toolCallId":"run","kind":"execute",
            "status":"in_progress","rawInput":{"command":"cargo test --token sk-hidden"}}),
    );
    assert_eq!(
        execution_of(&events).map(|(label, _)| label),
        Some("cargo test --token [redacted]")
    );
    let unknown = update(
        &mut translator("gemini-cli"),
        json!({"sessionUpdate":"tool_call","toolCallId":"run","kind":"execute","status":"in_progress"}),
    );
    assert_eq!(
        execution_of(&unknown).map(|(label, _)| label),
        Some("Command tool")
    );
}

#[test]
fn a_reported_exit_code_survives_later_frames_and_content_output_is_not_repeated_in_the_tail() {
    for raw_output in [json!({"exitCode": 2}), json!({"exit_code": 2})] {
        let mut translator = translator("gemini-cli");
        update(
            &mut translator,
            json!({"sessionUpdate":"tool_call","toolCallId":"run","kind":"execute",
                "title":"make","status":"in_progress","rawInput":{"command":"make"}}),
        );
        let reported = update(
            &mut translator,
            json!({"sessionUpdate":"tool_call_update","toolCallId":"run","rawOutput":raw_output,
                "content":[{"type":"content","content":{"type":"text","text":"boom"}}]}),
        );
        let (_, execution) = execution_of(&reported).expect("tool row");
        assert_eq!(execution.exit_code, Some(2));
        assert_eq!(execution.output_tail, None);
        let settled = update(
            &mut translator,
            json!({"sessionUpdate":"tool_call_update","toolCallId":"run","status":"failed"}),
        );
        assert_eq!(
            execution_of(&settled).expect("tool row").1.exit_code,
            Some(2)
        );
    }
    let mut unreported = translator("gemini-cli");
    let events = update(
        &mut unreported,
        json!({"sessionUpdate":"tool_call","toolCallId":"run","kind":"execute","status":"completed",
            "rawOutput":{"exitCode":"0"}}),
    );
    assert_eq!(execution_of(&events).expect("tool row").1.exit_code, None);
}

#[test]
fn emitted_empty_command_output_is_distinct_from_missing_or_unrecognized_output() {
    for (fields, expected) in [
        (json!({}), None),
        (json!({"rawOutput": {"output":"unsupported"}}), None),
        (json!({"content":[]}), None),
        (json!({"rawOutput":""}), Some("")),
        (
            json!({"content":[{"type":"content","content":{"type":"text","text":""}}]}),
            Some(""),
        ),
    ] {
        let mut frame = json!({"sessionUpdate":"tool_call","toolCallId":"command","kind":"execute","status":"completed"});
        frame
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let events = update(&mut translator("claude-code"), frame);
        let outputs: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AdapterEvent::Item {
                    payload: NormalizedPayload::ToolCall { output_summary, .. },
                    ..
                } => Some(output_summary.as_ref().map(|output| output.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(outputs, [expected]);
    }
}

#[test]
fn console_blocks_are_redacted_together_and_raw_output_retention_stays_bounded() {
    for (fields, expected) in [
        (
            json!({"content":[{"type":"content","content":{"type":"text","text":"```console\nBearer \n```"}},
            {"type":"text","text":"private-value"}]}),
            "Bearer [redacted]".to_owned(),
        ),
        (
            json!({"rawOutput":"x".repeat(70_000)}),
            format!(
                "[output truncated] {}",
                "x".repeat(65_536 - "[output truncated] ".len())
            ),
        ),
    ] {
        let mut frame = json!({"sessionUpdate":"tool_call","toolCallId":"command","kind":"execute","status":"failed"});
        frame
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let events = update(&mut translator("claude-code"), frame);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { output_summary: Some(output), .. }, ..
        } if output.as_str() == expected))
        );
    }
}

#[test]
fn raw_output_control_marker_is_redacted_before_retention_discards_its_context() {
    let events = update(
        &mut translator("claude-code"),
        json!({
            "sessionUpdate":"tool_call", "toolCallId":"command", "kind":"execute", "status":"completed",
            "rawOutput": format!("Bearer\u{1}{}private-value", " ".repeat(70_000))
        }),
    );
    let output = events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload:
                    NormalizedPayload::ToolCall {
                        output_summary: Some(output),
                        ..
                    },
                ..
            } => Some(output.as_str()),
            _ => None,
        })
        .unwrap();
    assert_eq!(output.len(), 65_536);
    assert!(output.starts_with("[output truncated] "));
    assert!(output.ends_with("[redacted]"));
    assert!(!output.contains("private-value"));
}

#[test]
fn raw_and_console_output_reject_nul_before_retention_discards_the_bearer_marker() {
    let output = format!("Bearer\u{0}{}private-value", " ".repeat(70_000));
    for fields in [
        json!({"rawOutput": output}),
        json!({"content":[{"type":"content","content":{"type":"text","text":format!("```console\n{output}\n```")}}]}),
    ] {
        let mut frame = json!({"sessionUpdate":"tool_call","toolCallId":"command","kind":"execute","status":"completed"});
        frame
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let result = translator("claude-code").translate_notification_value(
            json!({
                "method":"session/update", "params":{"sessionId":"session-1", "update": frame}
            }),
            &NoResources,
        );
        assert!(matches!(
            result,
            Err(super::translator::TranslationError::Contract(
                sage_acp_protocol::contract::ContractError::ForbiddenContent {
                    field: "toolOutput"
                }
            ))
        ));
    }
}

#[test]
fn direct_fenced_command_output_and_empty_read_output_keep_their_existing_semantics() {
    for (kind, content, expected) in [
        (
            "execute",
            json!([{"type":"text","text":"```console\nexample\n```"}]),
            Some("```console\nexample\n```"),
        ),
        (
            "read",
            json!([{"type":"content","content":{"type":"text","text":""}}]),
            None,
        ),
    ] {
        let events = update(
            &mut translator("opencode"),
            json!({
                "sessionUpdate":"tool_call", "toolCallId":"tool", "kind":kind, "status":"completed", "content":content
            }),
        );
        let outputs: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AdapterEvent::Item {
                    payload: NormalizedPayload::ToolCall { output_summary, .. },
                    ..
                } => Some(output_summary.as_ref().map(|output| output.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(outputs, [expected]);
    }
}
