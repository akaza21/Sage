use super::*;
use sage_core::limits::LIMITS;

async fn answer(pump: &mut Pump, text: &str) {
    for value in [
        json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
        json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":text}}),
        json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":text}]}}),
    ] {
        pump.push_stdout_chunk(&line(value), 10).await.unwrap();
    }
}

#[tokio::test]
async fn pi_pump_idle_model_and_thinking_changes_preserve_two_sqlite_turns() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    answer(&mut pump, "First answer").await;
    settle(&mut pump).await;
    let mut idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    for (id, value, data) in [
        ("pi.thinking", "off", Value::Null),
        (
            "pi.model",
            "anthropic:claude",
            json!({"provider":"anthropic","id":"claude","name":"Claude"}),
        ),
    ] {
        let mut transport = Capture::default();
        idle.write_session_config(&mut transport, &config::intent(id, value))
            .unwrap();
        idle.accept_idle_stdout_chunk(&reply(&transport.0[0], data), 30, &mut Capture::default())
            .await
            .unwrap();
        assert!(!idle.config_pending());
    }
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("second turn handoff"));
    start_turn(&mut pump, 1).await;
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(transport.0[0]["type"], "get_state");
    pump.push_stdout_chunk(
        &reply(
            &transport.0[0],
            json!({"model":{"provider":"anthropic","id":"claude"},"thinkingLevel":"off"}),
        ),
        32,
    )
    .await
    .unwrap();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(transport.0[1]["type"], "get_available_thinking_levels");
    pump.push_stdout_chunk(
        &reply(&transport.0[1], json!({"levels":["off","high"]})),
        33,
    )
    .await
    .unwrap();
    answer(&mut pump, "Second answer").await;
    settle(&mut pump).await;
    {
        let connection = fixture.database.connection();
        let turns: Vec<String> = connection
            .prepare("SELECT state FROM turns ORDER BY ordinal")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(turns, ["completed", "completed"]);
        let payloads: Vec<String> = connection
            .prepare("SELECT payload_json FROM items WHERE kind='text' ORDER BY turn_id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(payloads.len(), 2);
        assert!(payloads[0].contains("First answer"));
        assert!(payloads[1].contains("Second answer"));
        assert_eq!(
            connection
                .query_row("SELECT state FROM agent_sessions", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "active"
        );
    }
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_200_tool_steps_absorb_lifecycle_markers_under_item_budget() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    let mut absorbed = 0;
    for step in 0..200 {
        fixture.observed.lock().unwrap().clear();
        for value in content_free::tool_step(step) {
            pump.push_stdout_chunk(&line(value), 10).await.unwrap();
        }
        assert_eq!(fixture.item_count(), (step + 1) * 2);
        let events = fixture.observed.lock().unwrap().clone();
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AdapterEvent::Unsupported(_)))
        );
        absorbed += events
            .iter()
            .filter(|event| matches!(event, AdapterEvent::Absorbed))
            .count();
    }
    answer(&mut pump, "Inspected 200 files").await;
    settle(&mut pump).await;
    assert_eq!(fixture.item_count(), 401);
    assert!(fixture.item_count() < LIMITS.wire.items_per_turn);
    assert!(!pump.teardown_required());
    assert!(absorbed >= 200 * 25);
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_unknown_notices_are_bounded_and_reset_each_turn() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    for kind in 0..32 {
        for _ in 0..2 {
            pump.push_stdout_chunk(&line(json!({"type":format!("future_{kind}")})), 10)
                .await
                .unwrap();
        }
    }
    assert_eq!(fixture.item_count(), 32);
    settle(&mut pump).await;
    let idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("second turn handoff"));
    start_turn(&mut pump, 1).await;
    pump.push_stdout_chunk(&line(json!({"type":"future_0"})), 10)
        .await
        .unwrap();
    assert_eq!(fixture.item_count(), 33);
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_unknown_kind_capacity_suppresses_once_without_teardown() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    for kind in 0..32 {
        pump.push_stdout_chunk(&line(json!({"type":format!("future_{kind}")})), 10)
            .await
            .unwrap();
    }
    fixture.observed.lock().unwrap().clear();
    pump.push_stdout_chunk(&line(json!({"type":"future_over_capacity"})), 10)
        .await
        .unwrap();
    let events = fixture.observed.lock().unwrap().clone();
    assert!(
        matches!(events.as_slice(), [AdapterEvent::Unsupported(NormalizedPayload::Unsupported { external_kind, summary })] if external_kind.as_str() == "pi_unknown_event_limit" && summary.as_str().contains("suppressed"))
    );
    assert_eq!(fixture.item_count(), 33);
    fixture.observed.lock().unwrap().clear();
    for kind in 32..232 {
        for _ in 0..2 {
            pump.push_stdout_chunk(&line(json!({"type":format!("future_{kind}")})), 10)
                .await
                .unwrap();
        }
    }
    let events = fixture.observed.lock().unwrap().clone();
    assert!(
        events
            .iter()
            .all(|event| matches!(event, AdapterEvent::Absorbed))
    );
    assert!(!pump.teardown_required());
    assert_eq!(fixture.item_count(), 33);
    settle(&mut pump).await;
    let idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("second turn"));
    start_turn(&mut pump, 1).await;
    fixture.observed.lock().unwrap().clear();
    for kind in 0..34 {
        pump.push_stdout_chunk(&line(json!({"type":format!("future_{kind}")})), 10)
            .await
            .unwrap();
    }
    let events = fixture.observed.lock().unwrap().clone();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AdapterEvent::Unsupported(_)))
            .count(),
        33
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AdapterEvent::Absorbed))
            .count(),
        1
    );
    assert_eq!(fixture.item_count(), 66);
    settle(&mut pump).await;
    assert!(!pump.teardown_required());
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_unknown_and_lifecycle_frames_before_prompt_reply_are_nonfatal() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    pump.queue_command(AdapterCommand::Prompt {
        prompt: Prompt::new("prompt", "Inspect").unwrap(),
        client_message_id: RequiredText::new("client", "turn-pump-0").unwrap(),
        access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
    })
    .unwrap();
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    for row in [Row::Lifecycle, Row::Unknown] {
        for value in row_frames(row) {
            pump.push_stdout_chunk(&line(value), 1).await.unwrap();
        }
    }
    assert_eq!(fixture.item_count(), 2);
    pump.push_stdout_chunk(&reply(&transport.0[0], Value::Null), 2)
        .await
        .unwrap();
    pump.push_stdout_chunk(&line(json!({"type":"agent_start"})), 3)
        .await
        .unwrap();
    settle(&mut pump).await;
    fixture.finish().await;
}
