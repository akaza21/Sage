use super::*;
use sage_acp_protocol::{
    contract::SageChatId,
    scheduler::RuntimeOwner,
    session_config::{ConfigIntent, ConfigMutation, ConfigRevision, ConfigValue},
};

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

fn settings_updated(thread: &str, model: &str, effort: &str) -> Value {
    json!({"method":"thread/settings/updated","params":{"threadId":thread,"threadSettings":{
        "disabledPluginIds":[],"cwd":"C:\\repo","approvalPolicy":"never",
        "approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"},
        "activePermissionProfile":null,"model":model,"modelProvider":"openai",
        "serviceTier":null,"effort":effort,"summary":null,
        "collaborationMode":{"mode":"default","settings":{"model":model,
            "reasoning_effort":effort,"developer_instructions":null}},
        "personality":null
    }}})
}

fn turn_start(runtime: &mut CodexRuntime, client: &str) -> Value {
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "continue").unwrap(),
            client_message_id: RequiredText::new("client", client).unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
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
fn a_thread_on_an_unlisted_model_offers_its_own_model_beside_the_listed_ones() {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("hidden-model"), Some("medium"));
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(&frame(json!({"id":list_id,"result":catalogue()})), &Mapper)
        .unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    let model = option(&config, "model");
    assert_eq!(model["currentValue"], "hidden-model");
    assert_eq!(
        model["options"],
        json!([
            {"value":"hidden-model","name":"hidden-model"},
            {"value":"fixture-model","name":"Synthetic replay"},
            {"value":"fixture-model-deep","name":"Synthetic replay deep"}
        ])
    );
    runtime.configure(&pick("model", "hidden-model")).unwrap();
    assert!(turn_start(&mut runtime, "own").get("model").is_none());
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("hidden-model"), Some("medium"));
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(&frame(json!({"id":list_id,"result":catalogue()})), &Mapper)
        .unwrap();
    runtime.configure(&pick("model", "fixture-model")).unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(option(&config, "model")["currentValue"], "fixture-model");
    assert_eq!(option(&config, "effort")["currentValue"], "medium");
    let params = turn_start(&mut runtime, "listed");
    assert_eq!(params["model"], "fixture-model");
    assert_eq!(params["effort"], "medium");
}

#[test]
fn switching_to_a_model_always_sends_an_effort_that_model_offers() {
    for (reported, default, sent) in [
        (None, json!("high"), "high"),
        (Some("medium"), json!("max"), "low"),
        (Some("medium"), Value::Null, "low"),
    ] {
        let mut runtime = fixture_runtime();
        ready_with_reported_config(&mut runtime, Some("fixture-model"), reported);
        let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
        let mut catalogue = catalogue();
        catalogue["data"][1]["defaultReasoningEffort"] = default;
        catalogue["data"][1]["supportedReasoningEfforts"] = json!([
            {"reasoningEffort":"low","description":"Quick"},
            {"reasoningEffort":"high","description":"Deep"}
        ]);
        runtime
            .ingest(&frame(json!({"id":list_id,"result":catalogue})), &Mapper)
            .unwrap();
        runtime
            .configure(&pick("model", "fixture-model-deep"))
            .unwrap();
        let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
        assert_eq!(option(&config, "effort")["currentValue"], sent);
        let params = turn_start(&mut runtime, "switched");
        assert_eq!(params["model"], "fixture-model-deep");
        assert_eq!(params["effort"], sent);
    }
}

#[test]
fn a_reported_thread_settings_change_replaces_the_picked_model() {
    let mut runtime = hydrated();
    runtime
        .configure(&pick("model", "fixture-model-deep"))
        .unwrap();
    runtime
        .ingest(
            &frame(settings_updated("thread-1", "fixture-model", "high")),
            &Mapper,
        )
        .unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(option(&config, "model")["currentValue"], "fixture-model");
    assert_eq!(option(&config, "effort")["currentValue"], "high");
    assert_eq!(
        config["confirmedModel"],
        json!({"id":"fixture-model","name":"Synthetic replay"})
    );
    let params = turn_start(&mut runtime, "reported");
    assert!(params.get("model").is_none());
    assert_eq!(params["effort"], "high");
}

#[test]
fn the_config_is_hydrating_until_every_listed_page_is_answered() {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("fixture-model"), Some("medium"));
    assert!(!runtime.config_hydrating());
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    assert!(runtime.config_hydrating());
    let mut first = catalogue();
    first["nextCursor"] = json!("page-2");
    let batch = runtime
        .ingest(&frame(json!({"id":list_id,"result":first})), &Mapper)
        .unwrap();
    assert!(runtime.config_hydrating());
    assert_eq!(
        runtime
            .session_config()
            .unwrap()
            .config_options
            .as_slice()
            .len(),
        2
    );
    let next = request_id(&batch.egress()[0]);
    runtime
        .ingest(
            &frame(json!({"id":next,"error":{"code":-32601,"message":"Method not found"}})),
            &Mapper,
        )
        .unwrap();
    assert!(!runtime.config_hydrating());
}
