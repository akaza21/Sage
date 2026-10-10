use super::*;

#[tokio::test]
async fn pi_pump_ready_and_activated_adapters_settle_after_discovery() {
    let ready = ready_adapter();
    let activated = activated_adapter();
    assert_ne!(ready.capabilities(), activated.capabilities());
    for (adapter, discovery_pending) in [(ready, false), (activated, true)] {
        let fixture =
            Fixture::with_external_session_id(adapter.inner.external_session.clone().unwrap());
        let mut pump = fixture.pump(adapter);
        start(&mut pump).await;
        if discovery_pending {
            let transport = state_read(&mut pump, "openai", "gpt").await;
            pump.push_stdout_chunk(
                &reply(&transport.0[1], json!({"levels":["off","high"]})),
                31,
            )
            .await
            .unwrap();
        }
        let mut drained = Capture::default();
        pump.flush_one(&mut drained).unwrap();
        assert!(drained.0.is_empty());
        settle(&mut pump).await;
        fixture.finish().await;
    }
}

pub(super) async fn state_read(pump: &mut Pump, provider: &str, model: &str) -> Capture {
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(transport.0.len(), 1, "prompt must issue a state read");
    assert_eq!(transport.0[0]["type"], "get_state");
    pump.push_stdout_chunk(
        &reply(
            &transport.0[0],
            json!({"model":{"provider":provider,"id":model},"thinkingLevel":"high"}),
        ),
        30,
    )
    .await
    .unwrap();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(transport.0.len(), 2, "state reply must issue a levels read");
    assert_eq!(transport.0[1]["type"], "get_available_thinking_levels");
    transport
}

async fn discovered_option(adapter: ObservedPi) {
    let fixture =
        Fixture::with_external_session_id(adapter.inner.external_session.clone().unwrap());
    let mut pump = fixture.pump(adapter);
    start(&mut pump).await;
    let transport = state_read(&mut pump, "openai", "gpt").await;
    pump.push_stdout_chunk(
        &reply(&transport.0[1], json!({"levels":["high","max"]})),
        31,
    )
    .await
    .unwrap();
    let mut drained = Capture::default();
    pump.flush_one(&mut drained).unwrap();
    assert!(drained.0.is_empty());
    settle(&mut pump).await;
    let mut idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    let mut transport = Capture::default();
    idle.write_session_config(&mut transport, &config::intent("pi.thinking", "max"))
        .unwrap();
    assert_eq!(transport.0[0]["type"], "set_thinking_level");
    idle.accept_idle_stdout_chunk(
        &reply(&transport.0[0], Value::Null),
        32,
        &mut Capture::default(),
    )
    .await
    .unwrap();
    assert!(!idle.config_pending());
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("discovered session second turn"));
    start_turn(&mut pump, 1).await;
    let mut drained = Capture::default();
    pump.flush_one(&mut drained).unwrap();
    assert!(drained.0.is_empty(), "accepted levels remain available");
    settle(&mut pump).await;
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_discovers_thinking_levels_after_activation() {
    let activation = activation_with(activated_adapter(), false);
    let (adapter, _, _, _) = activation.parts.into_parts();
    discovered_option(adapter).await;
}

#[tokio::test]
async fn pi_pump_rediscovers_thinking_levels_after_new_session() {
    let activation = activation_idle(true);
    let (adapter, _, _, _) = activation.parts.into_parts();
    discovered_option(adapter).await;
}

#[tokio::test]
async fn pi_pump_model_refresh_retries_levels_after_fast_settlement() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    settle(&mut pump).await;
    let mut idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    let mut transport = Capture::default();
    idle.write_session_config(
        &mut transport,
        &config::intent("pi.model", "anthropic:claude"),
    )
    .unwrap();
    idle.accept_idle_stdout_chunk(
        &reply(
            &transport.0[0],
            json!({"provider":"anthropic","id":"claude"}),
        ),
        22,
        &mut Capture::default(),
    )
    .await
    .unwrap();
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("second turn"));
    start_turn(&mut pump, 1).await;
    let first = state_read(&mut pump, "anthropic", "claude").await;
    settle(&mut pump).await;
    let idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("fast idle handoff"));
    assert!(!idle.config_pending());
    fixture.add_turn(2);
    let mut pump = idle
        .bind_turn(fixture.intent(2).active_turn)
        .unwrap_or_else(|_| panic!("third turn"));
    start_turn(&mut pump, 2).await;
    let second = state_read(&mut pump, "anthropic", "claude").await;
    assert_ne!(first.0[1]["id"], second.0[1]["id"]);
    fixture.observed.lock().unwrap().clear();
    pump.push_stdout_chunk(&reply(&first.0[1], json!({"levels":["off","high"]})), 32)
        .await
        .unwrap();
    let events = fixture.observed.lock().unwrap().clone();
    assert!(matches!(events.as_slice(), [AdapterEvent::IgnoredLate]));
    assert!(
        pump.config_pending(),
        "stale reply must not clear the fresh discovery"
    );
    pump.push_stdout_chunk(&reply(&second.0[1], json!({"levels":["high","max"]})), 33)
        .await
        .unwrap();
    settle(&mut pump).await;
    let mut idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("final idle handoff"));
    let mut transport = Capture::default();
    idle.write_session_config(&mut transport, &config::intent("pi.thinking", "max"))
        .unwrap();
    idle.accept_idle_stdout_chunk(
        &reply(&transport.0[0], Value::Null),
        34,
        &mut Capture::default(),
    )
    .await
    .unwrap();
    assert!(!idle.config_pending());
    fixture.add_turn(3);
    let mut pump = idle
        .bind_turn(fixture.intent(3).active_turn)
        .unwrap_or_else(|_| panic!("fourth turn"));
    start_turn(&mut pump, 3).await;
    let mut drained = Capture::default();
    pump.flush_one(&mut drained).unwrap();
    assert!(
        drained.0.is_empty(),
        "accepted levels must finish the deferred refresh"
    );
    settle(&mut pump).await;
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_discovery_retries_state_read_after_fast_settlement() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(activated_adapter());
    start(&mut pump).await;
    let mut first = Capture::default();
    pump.flush_one(&mut first).unwrap();
    assert_eq!(first.0.len(), 1);
    assert_eq!(first.0[0]["type"], "get_state");
    settle(&mut pump).await;
    let idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("fast idle handoff"));
    assert!(!idle.config_pending());
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("second turn"));
    start_turn(&mut pump, 1).await;
    let second = state_read(&mut pump, "openai", "gpt").await;
    assert_ne!(first.0[0]["id"], second.0[0]["id"]);
    fixture.observed.lock().unwrap().clear();
    pump.push_stdout_chunk(
        &reply(
            &first.0[0],
            json!({"model":{"provider":"openai","id":"gpt"},"thinkingLevel":"off"}),
        ),
        32,
    )
    .await
    .unwrap();
    let events = fixture.observed.lock().unwrap().clone();
    assert!(matches!(events.as_slice(), [AdapterEvent::IgnoredLate]));
    assert!(pump.config_pending());
    pump.push_stdout_chunk(&reply(&second.0[1], json!({"levels":["high","max"]})), 33)
        .await
        .unwrap();
    assert!(!pump.config_pending());
    settle(&mut pump).await;
    assert!(!pump.teardown_required());
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_closed_discovery_does_not_block_idle_config() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(activated_adapter());
    start(&mut pump).await;
    let read = state_read(&mut pump, "openai", "gpt").await;
    settle(&mut pump).await;
    let mut idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    let mut transport = Capture::default();
    idle.write_session_config(&mut transport, &config::intent("pi.model", "openai:gpt"))
        .unwrap();
    idle.accept_idle_stdout_chunk(
        &reply(&transport.0[0], json!({"provider":"openai","id":"gpt"})),
        32,
        &mut Capture::default(),
    )
    .await
    .unwrap();
    fixture.observed.lock().unwrap().clear();
    idle.accept_idle_stdout_chunk(
        &reply(&read.0[1], json!({"levels":["off","high"]})),
        33,
        &mut Capture::default(),
    )
    .await
    .unwrap();
    let events = fixture.observed.lock().unwrap().clone();
    assert!(matches!(events.as_slice(), [AdapterEvent::IgnoredLate]));
    assert!(!idle.config_pending());
    fixture.finish().await;
}
