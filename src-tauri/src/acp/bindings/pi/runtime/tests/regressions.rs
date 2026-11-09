use super::*;
use crate::acp::{
    lifecycle::{ItemLifecycle, ToolState, TurnState},
    runtime::permissions::PermissionDecision,
};

#[test]
fn pi_0742_idless_optional_discovery_error_does_not_break_first_prompt() {
    let mut runtime = fixture_runtime(74);
    ready_until_levels(&mut runtime);
    running(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"response", "command":"get_available_thinking_levels",
                "success":false, "error":"Unknown command: get_available_thinking_levels"
            })),
            &Mapper,
        )
        .expect("optional discovery failure");
    assert!(batch.egress().is_empty());
    assert!(runtime.thinking_request_command.is_none());
    runtime
        .ingest(&frame(json!({"type":"agent_end","messages":[]})), &Mapper)
        .expect("first prompt still completes");
    let batch = runtime
        .ingest(&frame(json!({"type":"agent_settled"})), &Mapper)
        .unwrap();
    assert!(batch.events().iter().any(|event| matches!(
        event,
        AdapterEvent::Turn {
            state: TurnState::Completed,
            ..
        }
    )));
}

#[test]
fn pi_idless_optional_error_rejects_ambiguous_requests_for_the_method() {
    let mut runtime = fixture_runtime(76);
    ready_until_levels(&mut runtime);
    runtime
        .translator
        .begin(Operation::GetThinkingLevels)
        .unwrap();
    assert!(runtime.ingest(&frame(json!({"type":"response","command":"get_available_thinking_levels","success":false})), &Mapper).is_err());
}

#[test]
fn pi_idless_errors_outside_pending_optional_discovery_remain_fatal() {
    for (command, success) in [
        ("get_available_thinking_levels", false),
        ("get_commands", false),
        ("get_state", false),
        ("get_available_thinking_levels", true),
        ("get_commands", true),
    ] {
        let mut runtime = fixture_runtime(75);
        ready(&mut runtime);
        assert!(
            runtime
                .ingest(
                    &frame(json!({"type":"response","command":command,"success":success})),
                    &Mapper
                )
                .is_err()
        );
    }
}

fn thought_update(kind: &str, delta: Option<&str>) -> Value {
    let mut event = json!({"type":kind,"contentIndex":0});
    let partial = json!({"role":"assistant","content":[]});
    event["partial"] = partial.clone();
    if kind == "thinking_end" {
        event["content"] = json!(delta.unwrap_or_default());
    } else if let Some(delta) = delta {
        event["delta"] = json!(delta);
    }
    json!({"type":"message_update","message":partial,"assistantMessageEvent":event})
}

#[test]
fn pi_split_thought_secret_redacts_attached_continuation() {
    let mut runtime = fixture_runtime(63);
    ready(&mut runtime);
    running(&mut runtime);
    let mut rendered = String::new();
    for delta in ["sk-", "private", "-tail", " safe"] {
        let batch = runtime
            .ingest(
                &frame(thought_update("thinking_delta", Some(delta))),
                &Mapper,
            )
            .unwrap();
        rendered.push_str(&format!("{:?}", batch.events()));
    }
    assert!(!rendered.contains("private"));
    assert!(!rendered.contains("-tail"));
    assert!(rendered.contains("_ safe"));
}

#[test]
fn pi_literal_underscore_continuations_preserve_text_and_thought_content() {
    for kind in ["text_delta", "thinking_delta"] {
        let mut runtime = fixture_runtime(69);
        ready(&mut runtime);
        running(&mut runtime);
        runtime
            .ingest(
                &frame(json!({"type":"message_start","message":{"role":"assistant","content":[]}})),
                &Mapper,
            )
            .unwrap();
        runtime
            .ingest(
                &frame(thought_update(kind, Some("Checking snake_"))),
                &Mapper,
            )
            .unwrap();
        let batch = runtime
            .ingest(
                &frame(thought_update(kind, Some("case identifiers"))),
                &Mapper,
            )
            .unwrap();
        assert!(
            matches!(batch.events(), [AdapterEvent::Item {
            payload: NormalizedPayload::Text { text }, ..
        }] if text.as_str() == "Checking snake_case identifiers")
                || matches!(batch.events(), [AdapterEvent::Item {
            payload: NormalizedPayload::ThoughtSummary { summary }, ..
        }] if summary.as_str() == "Checking snake_case identifiers")
        );
    }
}

#[test]
fn pi_retry_continuation_keeps_turn_ownership_until_settlement() {
    assert_continuation(include_str!(
        "../../../../../../testdata/acp/pi/retry_continuation.jsonl"
    ));
}

#[test]
fn pi_compaction_continuation_keeps_turn_ownership_until_settlement() {
    assert_continuation(include_str!(
        "../../../../../../testdata/acp/pi/compaction_continuation.jsonl"
    ));
}

fn assert_continuation(fixture: &str) {
    let mut runtime = fixture_runtime(64);
    ready(&mut runtime);
    running(&mut runtime);
    let mut terminals = 0;
    let mut continued_text = false;
    for line in fixture.lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        let settled = value["type"] == "agent_settled";
        let session_notification = value["type"] == "entry_appended";
        let batch = runtime
            .ingest(&frame(value), &Mapper)
            .expect("continuation");
        assert!(
            session_notification
                || !batch
                    .events()
                    .iter()
                    .any(|event| matches!(event, AdapterEvent::IgnoredLate))
        );
        for event in batch.events() {
            if let AdapterEvent::Turn {
                state: TurnState::Completed,
                ..
            } = event
            {
                assert!(settled);
                terminals += 1;
            }
            if let AdapterEvent::Item {
                payload: NormalizedPayload::Text { text },
                ..
            } = event
            {
                continued_text |= text.as_str() == "Recovered";
            }
        }
        if !settled {
            assert!(matches!(
                runtime.command(AdapterCommand::Prompt {
                    prompt: Prompt::new("prompt", "Must not steal ownership").unwrap(),
                    client_message_id: RequiredText::new("client", "next").unwrap(),
                    access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
                }),
                Err(AdapterRuntimeError::Operation)
            ));
        }
    }
    assert!(continued_text);
    assert_eq!(terminals, 1);
}

#[test]
fn pi_compaction_outcomes_use_shared_completed_failed_and_cancelled_lifecycles() {
    let mut runtime = fixture_runtime(65);
    ready(&mut runtime);
    running(&mut runtime);
    let fixture = include_str!("../../../../../../testdata/acp/pi/compaction_outcomes.jsonl");
    let mut lifecycles = Vec::new();
    let mut ids = Vec::new();
    for line in fixture.lines() {
        let batch = runtime
            .ingest(&frame(serde_json::from_str(line).unwrap()), &Mapper)
            .unwrap();
        assert!(matches!(
            batch.events(),
            [AdapterEvent::Item {
                payload: NormalizedPayload::CompactionNotice,
                ..
            }]
        ));
        if let AdapterEvent::Item { id, lifecycle, .. } = &batch.events()[0] {
            lifecycles.push(*lifecycle);
            ids.push(id.external_id.clone());
        }
    }
    assert_eq!(
        lifecycles,
        [
            ItemLifecycle::Started,
            ItemLifecycle::Completed,
            ItemLifecycle::Started,
            ItemLifecycle::Cancelled,
            ItemLifecycle::Started,
            ItemLifecycle::Failed
        ]
    );
    assert_eq!(ids[0], ids[1]);
    assert_eq!(ids[2], ids[3]);
    assert_eq!(ids[4], ids[5]);
    assert_ne!(ids[0], ids[2]);
    for kind in ["auto_compaction_start", "auto_compaction_end"] {
        let batch = runtime
            .ingest(&frame(json!({"type":kind})), &Mapper)
            .unwrap();
        assert!(matches!(batch.events(), [AdapterEvent::Unsupported(_)]));
    }
}

#[test]
fn pi_thought_message_end_tool_next_thought_completes_distinct_items() {
    let mut runtime = fixture_runtime(61);
    ready(&mut runtime);
    running(&mut runtime);
    let mut completed = Vec::new();
    for event in [
        json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
        thought_update("thinking_start", None),
        thought_update("thinking_delta", Some("First thought")),
        json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"thinking","thinking":"First thought"}]}}),
        json!({"type":"tool_execution_start","toolCallId":"read-between","toolName":"read","args":{"path":"src/lib.rs"}}),
        json!({"type":"tool_execution_end","toolCallId":"read-between","toolName":"read","result":{"content":[{"type":"text","text":"read"}]},"isError":false}),
        json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
        thought_update("thinking_start", None),
        thought_update("thinking_delta", Some("Second thought")),
        thought_update("thinking_end", Some("Second thought")),
        json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Second thought"}]}}),
    ] {
        let batch = runtime
            .ingest(&frame(event), &Mapper)
            .expect("thought lifecycle");
        for event in batch.events() {
            if let AdapterEvent::Item {
                id,
                lifecycle: ItemLifecycle::Completed,
                payload: NormalizedPayload::ThoughtSummary { summary },
                ..
            } = event
            {
                completed.push((id.external_id.clone(), summary.as_str().to_owned()));
            }
        }
    }
    assert_eq!(completed.len(), 2);
    assert_ne!(completed[0].0, completed[1].0);
    assert_eq!(completed[0].1, "First thought");
    assert_eq!(completed[1].1, "Second thought");
}

#[test]
fn pi_interruption_cancels_the_live_thought_before_the_turn() {
    let mut runtime = fixture_runtime(62);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(
            &frame(thought_update("thinking_delta", Some("Working"))),
            &Mapper,
        )
        .unwrap();
    let cancel = runtime.command(AdapterCommand::Cancel).unwrap();
    deliver(&mut runtime, cancel);
    runtime
        .ingest(
            &frame(json!({"type":"agent_end","messages":[],"willRetry":false})),
            &Mapper,
        )
        .unwrap();
    let batch = runtime
        .ingest(&frame(json!({"type":"agent_settled"})), &Mapper)
        .unwrap();
    assert!(matches!(
        batch.events(),
        [
            AdapterEvent::Item {
                lifecycle: ItemLifecycle::Cancelled,
                payload: NormalizedPayload::ThoughtSummary { .. },
                ..
            },
            AdapterEvent::Turn {
                state: TurnState::Cancelled,
                ..
            }
        ]
    ));
}

#[test]
fn pi_aborted_assistant_message_end_cancels_its_thought() {
    let mut runtime = fixture_runtime(68);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(
            &frame(json!({"type":"message_start","message":{"role":"assistant","content":[]}})),
            &Mapper,
        )
        .unwrap();
    runtime
        .ingest(
            &frame(thought_update("thinking_delta", Some("Working"))),
            &Mapper,
        )
        .unwrap();
    let batch = runtime.ingest(&frame(json!({
        "type":"message_end","message":{"role":"assistant","content":[],"stopReason":"aborted"}
    })), &Mapper).unwrap();
    assert!(matches!(
        batch.events(),
        [
            AdapterEvent::Item {
                lifecycle: ItemLifecycle::Cancelled,
                payload: NormalizedPayload::ThoughtSummary { .. },
                ..
            },
            // The message never streamed text, so no empty answer appears.
            AdapterEvent::Absorbed
        ]
    ));
}

#[test]
fn pi_observed_write_is_declined_and_aborted_without_offering_approval() {
    let mut runtime = fixture_runtime(60);
    ready(&mut runtime);
    running(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"tool_execution_start", "toolCallId":"write-observed", "toolName":"write",
                "args":{"path":r"C:\repo\src\lib.rs", "content":"private"}
            })),
            &Mapper,
        )
        .expect("blocked observation");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Item {
            lifecycle: ItemLifecycle::Cancelled,
            tool_state: Some(ToolState::Declined),
            payload: NormalizedPayload::ToolCall { .. },
            ..
        }]
    ));
    assert_eq!(batch.egress().len(), 1);
    assert!(batch.egress()[0].token().is_mandatory());
    assert_eq!(request(&batch.egress()[0])["type"], "abort");
    assert!(matches!(
        runtime.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("write-observed").unwrap(),
            decision: PermissionDecision::AllowOnce,
        }),
        Err(AdapterRuntimeError::LaterPermissionMiss)
    ));
    // Sage's policy stopped the Turn, not the user, so it ends Interrupted.
    let abort = batch.into_parts().1.remove(0);
    runtime.commit_mandatory_egress(abort.token()).unwrap();
    let abort = deliver(&mut runtime, abort);
    runtime
        .ingest(&response(&abort, Value::Null), &Mapper)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"type":"agent_end","messages":[{"role":"assistant","content":[],"stopReason":"aborted"}]})),
            &Mapper,
        )
        .unwrap();
    let settled = runtime
        .ingest(&frame(json!({"type":"agent_settled"})), &Mapper)
        .unwrap();
    assert!(
        settled.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Interrupted,
                ..
            }
        )),
        "{:?}",
        settled.events()
    );
}

#[test]
fn pi_nonblocking_extension_notifications_never_offer_approval() {
    let mut runtime = fixture_runtime(66);
    ready(&mut runtime);
    running(&mut runtime);
    let batch = runtime.ingest(&frame(json!({"type":"extension_ui_request","id":"notify-1","method":"notify","message":"Working","notifyType":"info"})), &Mapper).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Session { .. }]));
    assert!(batch.egress().is_empty());
    let batch = runtime
        .ingest(
            &frame(json!({"type":"permission_request","id":"unproven"})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Unsupported(_)]));
    assert!(batch.egress().is_empty());
    let batch = runtime
        .ingest(
            &frame(thought_update("thinking_delta", Some("Still running"))),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ThoughtSummary { .. },
            ..
        }]
    ));
}

#[test]
fn pi_blocking_extension_confirm_allows_once_with_a_real_dialog_response() {
    let mut runtime = fixture_runtime(67);
    ready(&mut runtime);
    running(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"extension_ui_request","id":"gate-confirm","method":"confirm",
                "title":"Allow tool?","message":"Allow write?"
            })),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        batch.events(),
        [AdapterEvent::PermissionRequest { .. }]
    ));
    let reply = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("gate-confirm").unwrap(),
            decision: PermissionDecision::AllowOnce,
        })
        .unwrap();
    assert_eq!(
        request(&reply),
        json!({
            "type":"extension_ui_response","id":"gate-confirm","confirmed":true
        })
    );
    assert!(!reply.token().is_mandatory());
}
