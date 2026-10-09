use super::*;
use crate::acp::{
    lifecycle::{ItemLifecycle, ToolState},
    runtime::permissions::PermissionDecision,
};

#[test]
fn pi_thinking_level_changed_without_an_active_turn_is_nonfatal() {
    let mut runtime = fixture_runtime(70);
    for initialize_session in [false, true] {
        if initialize_session {
            ready(&mut runtime);
        }
        let batch = runtime
            .ingest(
                &frame(json!({
                    "type":"thinking_level_changed","level":"high"
                })),
                &Mapper,
            )
            .expect("idle session notification");
        assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
        assert!(batch.egress().is_empty());
    }
}

fn gate(id: &str) -> Value {
    json!({"type":"extension_ui_request","id":id,"method":"confirm",
        "title":"Allow tool?","message":"Allow write?","timeout":1})
}

#[test]
fn pi_timed_out_gate_does_not_hide_observed_authority_violation() {
    let mut runtime = fixture_runtime(72);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(&frame(gate("gate-expired")), &Mapper)
        .unwrap();
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"tool_execution_start","toolCallId":"write-after-timeout",
                "toolName":"write","args":{"path":"fixture.txt","content":"blocked"}
            })),
            &Mapper,
        )
        .expect("observation after dialog timeout");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Item {
            lifecycle: ItemLifecycle::Cancelled,
            tool_state: Some(ToolState::Declined),
            ..
        }]
    ));
    assert_eq!(batch.egress().len(), 1);
    assert!(batch.egress()[0].token().is_mandatory());
    assert_eq!(request(&batch.egress()[0])["type"], "abort");
    assert!(matches!(
        runtime.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("gate-expired").unwrap(),
            decision: PermissionDecision::AllowOnce,
        }),
        Err(AdapterRuntimeError::LaterPermissionMiss)
    ));
}

#[test]
fn pi_second_extension_gate_after_timeout_can_receive_a_reply() {
    let mut runtime = fixture_runtime(73);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(&frame(gate("gate-expired")), &Mapper)
        .unwrap();
    let batch = runtime.ingest(&frame(gate("gate-next")), &Mapper).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::PermissionRequest {
        external_request_id, ..
    }] if external_request_id.as_str() == "gate-next"));
    assert!(matches!(
        runtime.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("gate-expired").unwrap(),
            decision: PermissionDecision::AllowOnce,
        }),
        Err(AdapterRuntimeError::LaterPermissionMiss)
    ));
    let reply = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("gate-next").unwrap(),
            decision: PermissionDecision::AllowOnce,
        })
        .expect("current dialog response");
    assert_eq!(
        request(&reply),
        json!({
            "type":"extension_ui_response","id":"gate-next","confirmed":true
        })
    );
}

#[test]
fn pi_explicit_gate_denial_suppresses_continuation_and_further_gates() {
    let mut runtime = fixture_runtime(74);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(&frame(gate("gate-denied")), &Mapper)
        .unwrap();
    let abort = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("gate-denied").unwrap(),
            decision: PermissionDecision::Deny,
        })
        .unwrap();
    assert_eq!(request(&abort)["type"], "abort");
    for value in [
        gate("gate-too-late"),
        json!({"type":"tool_execution_start","toolCallId":"late-read","toolName":"read","args":{"path":"fixture.txt"}}),
    ] {
        let batch = runtime.ingest(&frame(value), &Mapper).unwrap();
        assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
        assert!(batch.egress().is_empty());
    }
    assert!(matches!(
        runtime.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("gate-denied").unwrap(),
            decision: PermissionDecision::AllowOnce,
        }),
        Err(AdapterRuntimeError::LaterPermissionMiss)
    ));
}

#[test]
fn pi_read_only_fixture_preserves_agent_loop_messages_and_followup() {
    let values: Vec<Value> = include_str!("../../../../../../testdata/acp/pi/read_only_turn.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let messages: Vec<Value> = values
        .iter()
        .filter(|value| value["type"] == "message_end")
        .map(|value| value["message"].clone())
        .collect();
    let end = values
        .iter()
        .find(|value| value["type"] == "agent_end")
        .unwrap();
    assert!(
        !values
            .iter()
            .any(|value| value["type"] == "tool_execution_update")
    );
    assert_eq!(end["messages"], json!(messages));
    let roles: Vec<&str> = messages
        .iter()
        .map(|message| message["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["user", "assistant", "toolResult", "assistant"]);
    assert_eq!(messages[1]["stopReason"], "toolUse");
    assert_eq!(messages[3]["stopReason"], "stop");
    assert!(
        messages[1]["content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|block| block["type"] == "toolCall" && block["id"] == "tool-1")
    );

    let mut runtime = fixture_runtime(75);
    ready(&mut runtime);
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect read-only").unwrap(),
            client_message_id: RequiredText::new("client", "fixture-followup").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt = deliver(&mut runtime, prompt);
    runtime
        .ingest(&response(&prompt, Value::Null), &Mapper)
        .unwrap();
    let mut texts = Vec::new();
    for value in values {
        let batch = runtime
            .ingest(&frame(value), &Mapper)
            .expect("source-shaped fixture");
        for event in batch.events() {
            if let AdapterEvent::Item {
                id,
                lifecycle: ItemLifecycle::Completed,
                payload: NormalizedPayload::Text { text },
                ..
            } = event
            {
                texts.push((id.external_id.clone(), text.as_str().to_owned()));
            }
        }
    }
    assert_eq!(texts.len(), 2);
    assert_ne!(texts[0].0, texts[1].0);
    assert_eq!(texts[0].1, "I will inspect the project.");
    assert_eq!(texts[1].1, "I inspected the project.");
}

#[test]
fn pi_select_gate_recognition_and_reply_share_exact_allow_options() {
    let mut runtime = fixture_runtime(76);
    ready(&mut runtime);
    running(&mut runtime);
    for (ordinal, option) in ["allow", "ALLOW", "Allow Once", "allow once"]
        .into_iter()
        .enumerate()
    {
        let id = format!("gate-option-{ordinal}");
        let batch = runtime
            .ingest(
                &frame(json!({
                    "type":"extension_ui_request","id":id,"method":"select",
                    "title":"Allow tool?","options":["Deny",option]
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
                request_id: SageServerRequestId::parse(id.clone()).unwrap(),
                decision: PermissionDecision::AllowOnce,
            })
            .unwrap();
        assert_eq!(
            request(&reply),
            json!({
                "type":"extension_ui_response","id":id,"value":option
            })
        );
    }
}

#[test]
fn pi_thinking_discovery_queue_rollback_allows_a_fresh_request() {
    let mut runtime = fixture_runtime(77);
    let first = ready_until_levels(&mut runtime);
    runtime
        .ingest(&response(&first, json!({"levels":["off","high"]})), &Mapper)
        .unwrap();
    let rejected = runtime.begin_thinking_discovery().unwrap();
    runtime
        .resolve_egress(rejected.into_token(), EgressResolution::QueueRejected)
        .unwrap();
    let next = runtime
        .begin_thinking_discovery()
        .expect("fresh discovery after rollback");
    let next = deliver(&mut runtime, next);
    let batch = runtime
        .ingest(&response(&next, json!({"levels":["off","high"]})), &Mapper)
        .unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredDuplicate]));
}
