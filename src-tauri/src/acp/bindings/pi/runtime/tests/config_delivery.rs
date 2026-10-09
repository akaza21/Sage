use super::*;
use crate::acp::{
    contract::{SageChatId, SageProjectId},
    scheduler::RuntimeOwner,
    session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigValue},
};

fn config_intent(id: &str, value: &str) -> ConfigIntent {
    ConfigIntent {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-pi").unwrap(),
            chat_id: SageChatId::parse("chat-pi").unwrap(),
            session_id: SageAgentSessionId::parse("session-pi-92").unwrap(),
            runtime_epoch: RuntimeEpoch::new(92).unwrap(),
        },
        expected_config_revision: serde_json::from_value(json!("1")).unwrap(),
        mutation: ConfigMutation::Option {
            config_id: ConfigId::new("config", id).unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", value).unwrap(),
            },
        },
    }
}

fn direct_config_delivery(id: &str, value: &str, data: Value) {
    let mut runtime = fixture_runtime(92);
    ready(&mut runtime);
    let egress = runtime
        .configure(&config_intent(id, value))
        .unwrap()
        .expect("Pi config requires a wire command");
    let command = request(&egress);
    let token = egress.into_token();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Written)
        .unwrap();
    assert_eq!(
        runtime.resolve_egress(token, EgressResolution::Written),
        Err(AdapterRuntimeError::EgressIntent)
    );
    let batch = runtime.ingest(&response(&command, data), &Mapper).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredDuplicate]));
    assert!(batch.egress().is_empty());
    assert!(!runtime.config_pending());
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert!(
        config["configOptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| option["id"] == id && option["currentValue"] == value)
    );
}

#[test]
fn pi_idle_model_config_accepts_direct_written_delivery_once() {
    direct_config_delivery(
        "pi.model",
        "openai:gpt",
        json!({"provider":"openai","id":"gpt","name":"GPT"}),
    );
}

#[test]
fn pi_idle_thinking_config_accepts_direct_written_delivery_once() {
    direct_config_delivery("pi.thinking", "off", Value::Null);
}

#[test]
fn pi_idle_config_zero_byte_delivery_rolls_back_for_retry() {
    for (id, value) in [("pi.model", "openai:gpt"), ("pi.thinking", "off")] {
        let mut runtime = fixture_runtime(92);
        ready(&mut runtime);
        let intent = config_intent(id, value);
        let egress = runtime
            .configure(&intent)
            .unwrap()
            .expect("Pi config requires a wire command");
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::WriteZeroBytes)
            .unwrap();
        assert!(!runtime.config_pending());
        let retry = runtime
            .configure(&intent)
            .unwrap()
            .expect("Pi config retry requires a wire command");
        runtime
            .resolve_egress(retry.into_token(), EgressResolution::Written)
            .unwrap();
    }
}

#[test]
fn pi_prompt_delivery_still_requires_queued_resolution() {
    let mut runtime = fixture_runtime(92);
    ready(&mut runtime);
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect").unwrap(),
            client_message_id: RequiredText::new("client", "direct-config-only").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let token = egress.into_token();
    assert_eq!(
        runtime.resolve_egress(token.clone(), EgressResolution::Written),
        Err(AdapterRuntimeError::EgressIntent)
    );
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .unwrap();
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
}
