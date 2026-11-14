use super::*;

pub(super) fn updates() -> Vec<Value> {
    [
        "text_start",
        "text_end",
        "toolcall_start",
        "toolcall_delta",
        "toolcall_end",
    ]
    .into_iter()
    .map(|kind| {
        let mut event = json!({"type":kind,"contentIndex":0});
        if kind == "toolcall_delta" {
            event["delta"] = json!("{}");
        }
        update(event, assistant(json!([]), 1, "stop"))
    })
    .collect()
}

pub(super) fn notifications() -> Vec<Value> {
    vec![
        json!({"type":"extension_ui_request","id":"status","method":"setStatus","statusKey":"loop","statusText":"Reading"}),
        json!({"type":"extension_ui_request","id":"widget","method":"setWidget","widgetKey":"loop","widgetLines":["Reading"]}),
        json!({"type":"extension_ui_request","id":"title","method":"setTitle","title":"Reading"}),
        json!({"type":"extension_ui_request","id":"editor","method":"set_editor_text","text":"Reading"}),
    ]
}

fn assistant(content: Value, timestamp: u64, stop: &str) -> Value {
    json!({"role":"assistant","content":content,"api":"openai-responses","provider":"openai","model":"fixture-model","usage":{"input":12,"output":7,"cacheRead":0,"cacheWrite":0,"totalTokens":19,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"stopReason":stop,"timestamp":timestamp})
}

fn update(event: Value, message: Value) -> Value {
    json!({"type":"message_update","usage":message["usage"],"assistantMessageEvent":event})
}

pub(super) fn tool_step(step: u64) -> Vec<Value> {
    let id = format!("read-loop-{step}");
    let text = json!({"type":"text","text":"Inspecting"});
    let call = json!({"type":"toolCall","id":id,"name":"read","arguments":{"path":"src/lib.rs"}});
    let partial = assistant(json!([text]), step * 2 + 1, "stop");
    let message = assistant(json!([text, call]), step * 2 + 1, "toolUse");
    let mut tool_partial = message.clone();
    tool_partial["content"][1]["arguments"] = json!({});
    let result = json!({"role":"toolResult","toolCallId":id,"toolName":"read","content":[{"type":"text","text":"Read"}],"details":{},"isError":false,"timestamp":step * 2 + 2});
    let mut frames = vec![
        json!({"type":"turn_start"}),
        json!({"type":"message_start","message":assistant(json!([]), step * 2 + 1, "stop")}),
        update(
            json!({"type":"text_start","contentIndex":0}),
            assistant(json!([{"type":"text","text":""}]), step * 2 + 1, "stop"),
        ),
        update(
            json!({"type":"text_delta","contentIndex":0,"delta":"Inspecting"}),
            partial.clone(),
        ),
        update(
            json!({"type":"text_end","contentIndex":0,"content":"Inspecting"}),
            partial,
        ),
        update(
            json!({"type":"toolcall_start","contentIndex":1}),
            tool_partial.clone(),
        ),
    ];
    let mut arguments = String::new();
    for fragment in r#"{"path":"src/lib.rs"}"#.chars() {
        arguments.push(fragment);
        if let Ok(value) = serde_json::from_str::<Value>(&arguments) {
            tool_partial["content"][1]["arguments"] = value;
        }
        frames.push(update(
            json!({"type":"toolcall_delta","contentIndex":1,"delta":fragment.to_string()}),
            tool_partial.clone(),
        ));
    }
    frames.extend([
        update(json!({"type":"toolcall_end","contentIndex":1,"toolCall":call}), message.clone()),
        json!({"type":"message_end","message":message}),
        json!({"type":"extension_ui_request","id":format!("status-{step}"),"method":"setStatus","statusKey":"loop","statusText":"Reading"}),
        json!({"type":"extension_ui_request","id":format!("notify-{step}"),"method":"notify","message":"Reading","notifyType":"info"}),
        tool_start(&id),
        json!({"type":"tool_execution_update","toolCallId":id,"toolName":"read","args":{"path":"src/lib.rs"},"partialResult":{"content":[{"type":"text","text":"Read"}],"details":{}}}),
        tool_end(&id, json!([{"type":"text","text":"Read"}])),
        json!({"type":"message_start","message":result}),
        json!({"type":"message_end","message":result}),
        json!({"type":"turn_end","message":message,"toolResults":[result]}),
    ]);
    frames
}

#[tokio::test]
async fn pi_pump_content_free_updates_idle() {
    exercise(Row::ContentFree, State::Idle).await;
}
#[tokio::test]
async fn pi_pump_content_free_updates_live() {
    exercise(Row::ContentFree, State::Live).await;
}
#[tokio::test]
async fn pi_pump_content_free_updates_closed() {
    exercise(Row::ContentFree, State::Closed).await;
}
#[tokio::test]
async fn pi_pump_fire_and_forget_ui_idle() {
    exercise(Row::FireAndForget, State::Idle).await;
}
#[tokio::test]
async fn pi_pump_fire_and_forget_ui_live() {
    exercise(Row::FireAndForget, State::Live).await;
}
#[tokio::test]
async fn pi_pump_fire_and_forget_ui_closed() {
    exercise(Row::FireAndForget, State::Closed).await;
}

#[tokio::test]
async fn pi_pump_read_only_fixture_preserves_real_stream_content() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    pump.queue_command(AdapterCommand::Prompt {
        prompt: Prompt::new("prompt", "Inspect read-only").unwrap(),
        client_message_id: RequiredText::new("client", "turn-pump-0").unwrap(),
        access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
    })
    .unwrap();
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    pump.push_stdout_chunk(&reply(&transport.0[0], Value::Null), 1)
        .await
        .unwrap();
    for record in include_str!("../../../../../../../testdata/acp/pi/read_only_turn.jsonl").lines()
    {
        let value: Value = serde_json::from_str(record).unwrap();
        pump.push_stdout_chunk(&line(value), 2).await.unwrap();
    }
    assert!(pump.completed_turn_ready());
    assert!(!pump.teardown_required());
    assert_eq!(fixture.item_count(), 3);
    let events = fixture.observed.lock().unwrap().clone();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AdapterEvent::Unsupported(_)))
    );
    assert!(events.iter().any(|event| matches!(event, AdapterEvent::Item { payload: NormalizedPayload::Text { text }, .. } if text.as_str() == "I inspected the project.")));
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_content_free_frames_remain_absorbed_until_settlement() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    pump.queue_command(AdapterCommand::Prompt {
        prompt: Prompt::new("prompt", "Inspect").unwrap(),
        client_message_id: RequiredText::new("client", "turn-pump-0").unwrap(),
        access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
    })
    .unwrap();
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    for phase in 0..3 {
        if phase == 1 {
            pump.push_stdout_chunk(&reply(&transport.0[0], Value::Null), 1)
                .await
                .unwrap();
            pump.push_stdout_chunk(&line(json!({"type":"agent_start"})), 2)
                .await
                .unwrap();
        } else if phase == 2 {
            pump.push_stdout_chunk(&line(json!({"type":"agent_end","messages":[]})), 3)
                .await
                .unwrap();
        }
        fixture.observed.lock().unwrap().clear();
        for value in updates().into_iter().chain(notifications()) {
            pump.push_stdout_chunk(&line(value), 4).await.unwrap();
        }
        let events = fixture.observed.lock().unwrap().clone();
        assert_eq!(events.len(), 9);
        assert!(
            events
                .iter()
                .all(|event| matches!(event, AdapterEvent::Absorbed))
        );
        assert_eq!(fixture.item_count(), 0);
        assert!(!pump.completed_turn_ready());
        assert!(!pump.teardown_required());
    }
    pump.push_stdout_chunk(&line(json!({"type":"agent_settled"})), 5)
        .await
        .unwrap();
    assert!(pump.completed_turn_ready());
    fixture.finish().await;
}
