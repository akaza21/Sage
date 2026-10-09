use super::*;
use crate::acp::session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigValue};

pub(super) fn intent(id: &str, value: &str) -> ConfigIntent {
    ConfigIntent {
        owner: owner(),
        expected_config_revision: serde_json::from_value(json!("1")).unwrap(),
        mutation: ConfigMutation::Option {
            config_id: ConfigId::new("config", id).unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", value).unwrap(),
            },
        },
    }
}

pub(super) fn pending_replies(runtime: &mut PiRuntime) -> Vec<Vec<u8>> {
    let levels = runtime.begin_thinking_discovery().unwrap();
    let levels = deliver(runtime, levels);
    let model = runtime
        .begin_one(
            Operation::SetModel {
                provider: "openai".into(),
                id: "gpt".into(),
            },
            RequestKind::SetModel,
        )
        .unwrap();
    let model = deliver(runtime, model);
    let thinking = runtime
        .begin_one(
            Operation::SetThinkingLevel {
                level: "off".into(),
            },
            RequestKind::SetThinkingLevel,
        )
        .unwrap();
    let thinking = deliver(runtime, thinking);
    vec![
        reply(&thinking, Value::Null),
        reply(&model, json!({"provider":"openai","id":"gpt","name":"GPT"})),
        reply(&levels, json!({"levels":["off","high"]})),
    ]
}

pub(super) async fn exercise(state: State) {
    if matches!(state, State::Idle) {
        for reset in [false, true] {
            let mut activation = activation_idle(reset);
            let (mut adapter, _, _, _) = activation.parts.into_parts();
            let levels = adapter.inner.begin_thinking_discovery().unwrap();
            let levels = deliver(&mut adapter.inner, levels);
            activation = activation_with(adapter, false);
            activation
                .accept_idle_stdout_chunk(&reply(&levels, json!({"levels":["off","high"]})))
                .unwrap();
            for (id, value, data) in [
                ("pi.thinking", "off", Value::Null),
                (
                    "pi.model",
                    "openai:gpt",
                    json!({"provider":"openai","id":"gpt","name":"GPT"}),
                ),
            ] {
                let mut transport = Capture::default();
                activation
                    .parts
                    .write_session_config(&mut transport, &intent(id, value))
                    .unwrap();
                activation
                    .accept_idle_stdout_chunk(&reply(&transport.0[0], data))
                    .unwrap();
                assert!(!activation.parts.config_pending());
            }
        }
    }
    let fixture = Fixture::new();
    let mut adapter = ready_adapter();
    let replies = pending_replies(&mut adapter.inner);
    let mut pump = fixture.pump(adapter);
    start(&mut pump).await;
    if !matches!(state, State::Live) {
        settle(&mut pump).await;
    }
    fixture.observed.lock().unwrap().clear();
    if matches!(state, State::Idle) {
        let mut idle = pump
            .try_into_completed_parts()
            .unwrap_or_else(|_| panic!("idle handoff"));
        for bytes in replies {
            idle.accept_idle_stdout_chunk(&bytes, 30).await.unwrap();
        }
        for (id, value, data) in [
            ("pi.thinking", "off", Value::Null),
            (
                "pi.model",
                "openai:gpt",
                json!({"provider":"openai","id":"gpt","name":"GPT"}),
            ),
        ] {
            let mut transport = Capture::default();
            idle.write_session_config(&mut transport, &intent(id, value))
                .unwrap();
            idle.accept_idle_stdout_chunk(&reply(&transport.0[0], data), 31)
                .await
                .unwrap();
            assert!(!idle.config_pending());
        }
    } else {
        for bytes in replies {
            pump.push_stdout_chunk(&bytes, 30).await.unwrap();
        }
        assert!(!pump.teardown_required());
    }
    let events = fixture.observed.lock().unwrap().clone();
    assert_eq!(
        events.len(),
        if matches!(state, State::Idle) { 5 } else { 3 }
    );
    if matches!(state, State::Closed) {
        assert!(
            events
                .iter()
                .all(|event| matches!(event, AdapterEvent::IgnoredLate))
        );
    } else {
        assert!(events.iter().all(|event| matches!(
            event,
            AdapterEvent::IgnoredDuplicate | AdapterEvent::IgnoredLate
        )));
    }
    assert_eq!(fixture.item_count(), 0);
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_idle_thinking_discovery_failures_keep_session_alive() {
    for malformed in [false, true] {
        let mut adapter = ready_adapter();
        let command = adapter.inner.begin_thinking_discovery().unwrap();
        let command = deliver(&mut adapter.inner, command);
        let mut idle = activation_with(adapter, false);
        let bytes = if malformed {
            reply(&command, json!({"levels":"invalid"}))
        } else {
            line(
                json!({"type":"response","id":command["id"],"command":command["type"],"success":false}),
            )
        };
        idle.accept_idle_stdout_chunk(&bytes).unwrap();
        assert!(!idle.parts.config_pending());
        let (adapter, _, _, _) = idle.parts.into_parts();
        assert!(adapter.session_config().is_some());
        let fixture = Fixture::new();
        let mut pump = fixture.pump(adapter);
        start(&mut pump).await;
        settle(&mut pump).await;
        fixture.finish().await;
    }
}

#[tokio::test]
async fn pi_pump_idle_config_refusals_preserve_session() {
    for (id, value) in [("pi.thinking", "off"), ("pi.model", "anthropic:claude")] {
        let mut idle = activation_idle(false);
        let mut transport = Capture::default();
        idle.parts
            .write_session_config(&mut transport, &intent(id, value))
            .unwrap();
        let command = &transport.0[0];
        assert!(matches!(idle.accept_idle_stdout_chunk(&line(json!({"type":"response","id":command["id"],"command":command["type"],"success":false}))),Err(crate::acp::runtime::AdapterPumpError::Refused(_))));
        assert!(!idle.parts.config_pending());
        let (adapter, _, _, _) = idle.parts.into_parts();
        let fixture = Fixture::new();
        let mut pump = fixture.pump(adapter);
        start(&mut pump).await;
        settle(&mut pump).await;
        fixture.finish().await;
    }
}
