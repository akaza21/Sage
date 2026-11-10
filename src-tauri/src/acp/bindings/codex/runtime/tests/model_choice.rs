//! A Codex thread's model as a session config choice: `model/list` names the
//! models the thread can run, a pick goes through the same config seam as
//! effort, and the next `turn/start` carries it, since Codex applies a
//! `turn/start` model to that Turn and the ones after it.
use super::*;
use crate::acp::{
    contract::SageChatId,
    scheduler::RuntimeOwner,
    session_config::{ConfigIntent, ConfigMutation, ConfigRevision, ConfigValue},
};

/// The adversarial fixture's restart catalogue: the default model with
/// three efforts, and a second model with the same efforts.
fn catalogue() -> Value {
    let efforts = json!([
        {"reasoningEffort":"low","description":"Quick synthetic replay"},
        {"reasoningEffort":"medium","description":"Balanced synthetic replay"},
        {"reasoningEffort":"high","description":"Thorough synthetic replay"}
    ]);
    json!({"data":[
        {"id":"fixture-model","model":"fixture-model","displayName":"Synthetic replay",
         "isDefault":true,"hidden":false,"defaultReasoningEffort":"medium",
         "supportedReasoningEfforts":efforts},
        {"id":"fixture-model-deep","model":"fixture-model-deep",
         "displayName":"Synthetic replay deep","isDefault":false,"hidden":false,
         "defaultReasoningEffort":"medium","supportedReasoningEfforts":efforts}
    ],"nextCursor":null})
}

/// A thread running `fixture-model` at Medium, with the catalogue hydrated.
fn hydrated() -> CodexRuntime {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("fixture-model"), Some("medium"));
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(&frame(json!({"id":list_id,"result":catalogue()})), &Mapper)
        .unwrap();
    runtime
}

fn pick(config_id: &str, value: &str) -> ConfigIntent {
    ConfigIntent {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-1").unwrap(),
            chat_id: SageChatId::parse("chat-1").unwrap(),
            session_id: SageAgentSessionId::parse("session-codex-runtime").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        },
        expected_config_revision: ConfigRevision::first(),
        mutation: ConfigMutation::Option {
            config_id: RequiredText::new("configId", config_id).unwrap(),
            value: ConfigValue::Select {
                value: RequiredText::new("value", value).unwrap(),
            },
        },
    }
}

fn option(config: &Value, category: &str) -> Value {
    config["configOptions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["category"] == category)
        .cloned()
        .unwrap_or(Value::Null)
}

fn turn_start(runtime: &mut CodexRuntime, client: &str) -> Value {
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "continue").unwrap(),
            client_message_id: RequiredText::new("client", client).unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let request: Value = serde_json::from_slice(prompt.frame().payload()).unwrap();
    assert_eq!(request["method"], "turn/start");
    request["params"].clone()
}

#[test]
fn the_thread_model_is_a_choice_among_the_listed_models() {
    let runtime = hydrated();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    let model = option(&config, "model");
    assert_eq!(model["type"], "select");
    assert_eq!(model["currentValue"], "fixture-model");
    assert_eq!(
        model["options"],
        json!([
            {"value":"fixture-model","name":"Synthetic replay"},
            {"value":"fixture-model-deep","name":"Synthetic replay deep"}
        ])
    );
    assert_eq!(option(&config, "effort")["currentValue"], "medium");
}

#[test]
fn a_picked_model_is_shown_with_its_efforts_and_reaches_the_next_turn_start() {
    // Without a pick, Codex keeps the thread's own model.
    assert!(
        turn_start(&mut hydrated(), "unpicked")
            .get("model")
            .is_none()
    );
    let mut runtime = hydrated();
    runtime
        .configure(&pick("model", "fixture-model-deep"))
        .unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(
        option(&config, "model")["currentValue"],
        "fixture-model-deep"
    );
    assert_eq!(
        config["confirmedModel"],
        json!({"id":"fixture-model-deep","name":"Synthetic replay deep"})
    );
    // The effort the thread ran is offered by the new model, so it stays.
    assert_eq!(option(&config, "effort")["currentValue"], "medium");
    runtime.configure(&pick("effort", "high")).unwrap();
    let params = turn_start(&mut runtime, "after");
    assert_eq!(params["model"], "fixture-model-deep");
    assert_eq!(params["effort"], "high");
}

#[test]
fn a_model_outside_the_listing_or_during_a_turn_is_refused() {
    let mut runtime = hydrated();
    assert!(runtime.configure(&pick("model", "unlisted")).is_err());
    running_turn(&mut runtime);
    assert!(
        runtime
            .configure(&pick("model", "fixture-model-deep"))
            .is_err()
    );
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(option(&config, "model")["currentValue"], "fixture-model");
}

#[test]
fn a_picked_model_without_the_current_effort_takes_its_own_default() {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("fixture-model"), Some("high"));
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    let mut catalogue = catalogue();
    catalogue["data"][1]["defaultReasoningEffort"] = json!("low");
    catalogue["data"][1]["supportedReasoningEfforts"] =
        json!([{"reasoningEffort":"low","description":"Quick"}]);
    runtime
        .ingest(&frame(json!({"id":list_id,"result":catalogue})), &Mapper)
        .unwrap();
    runtime
        .configure(&pick("model", "fixture-model-deep"))
        .unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(option(&config, "effort")["currentValue"], "low");
    assert_eq!(config["confirmedEffort"], "low");
    let params = turn_start(&mut runtime, "default-effort");
    assert_eq!(params["model"], "fixture-model-deep");
    assert_eq!(params["effort"], "low");
}

#[test]
fn a_thread_on_an_unlisted_model_offers_no_model_choice() {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("hidden-model"), Some("medium"));
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(&frame(json!({"id":list_id,"result":catalogue()})), &Mapper)
        .unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(option(&config, "model"), Value::Null);
    assert!(runtime.configure(&pick("model", "fixture-model")).is_err());
}
