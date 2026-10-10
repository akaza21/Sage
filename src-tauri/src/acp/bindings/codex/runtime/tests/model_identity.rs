use super::*;

#[test]
fn oversized_combined_config_preserves_identity_through_native_config_channel() {
    for (name, option_count) in [("Short".to_owned(), 1), ("M".repeat(512), 0)] {
        let mut adapter = fixture_runtime();
        initialize(&mut adapter);
        let owner = crate::acp::scheduler::RuntimeOwner {
            project_id: SageProjectId::parse("project-model-budget").unwrap(),
            chat_id: crate::acp::contract::SageChatId::parse("chat-model-budget").unwrap(),
            session_id: SageAgentSessionId::parse("session-codex-runtime").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        let channel = std::sync::Arc::new(crate::acp::session_config::ConfigChannel::default());
        channel.reserve(owner.clone());
        channel.publish(&owner);
        let mut runtime = crate::acp::bindings::runtime::NativeRuntime::new(adapter);
        runtime.attach_config(owner.clone(), std::sync::Arc::clone(&channel));
        let start = runtime
            .command(AdapterCommand::NewSession {
                native_root: r"C:\repo".into(),
                model: None,
            })
            .unwrap();
        runtime
            .ingest(
                &frame(json!({"id":request_id(&start),"result":{
                    "thread":{"id":"thread-1"},"model":"actual","reasoningEffort":"effort-0"
                }})),
                &Mapper,
            )
            .unwrap();
        let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
        let efforts: Vec<_> = (0..16)
            .map(|index| {
                json!({
                    "reasoningEffort":format!("effort-{index}"),"description":"d".repeat(8_092)
                })
            })
            .collect();
        runtime
            .ingest(
                &frame(json!({"id":list_id,"result":{"data":[{
                "id":"actual","displayName":name,
                "supportedReasoningEfforts":efforts
            }],"nextCursor":null}})),
                &Mapper,
            )
            .unwrap();
        let snapshot = serde_json::to_value(channel.live_snapshot(&owner).unwrap()).unwrap();
        assert_eq!(
            snapshot["confirmedModel"],
            json!({"id":"actual","name":name})
        );
        assert_eq!(
            snapshot["configOptions"].as_array().unwrap().len(),
            option_count
        );
        assert_eq!(snapshot["configRevision"], "2");
    }
}

#[test]
fn session_identity_uses_reported_model_not_requested_model_or_catalog_default() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("modelId", "requested").unwrap()),
        })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request_id(&start),"result":{
                "thread":{"id":"thread-1"},"model":"actual","reasoningEffort":"high"
            }})),
            &Mapper,
        )
        .unwrap();
    let before = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(before["confirmedModel"], json!({"id":"actual"}));
    assert_eq!(before["configOptions"], json!([]));
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[
                {"id":"default","displayName":"Default catalog model","isDefault":true},
                {"id":"actual","displayName":"Actual model","isDefault":false,
                 "defaultReasoningEffort":"low","supportedReasoningEfforts":[
                    {"reasoningEffort":"low","description":"Quick"},
                    {"reasoningEffort":"high","description":"Deep"}
                 ]}
            ],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(
        config["confirmedModel"],
        json!({"id":"actual","name":"Actual model"})
    );
    assert_eq!(config["configOptions"][0]["currentValue"], "high");
    assert_eq!(config["configOptions"].as_array().unwrap().len(), 1);
}

#[test]
fn unknown_session_model_does_not_become_the_catalog_default() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request_id(&start),"result":{"thread":{"id":"thread-1"}}})),
            &Mapper,
        )
        .unwrap();
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[
                {"id":"default","displayName":"Default catalog model","isDefault":true,
                 "defaultReasoningEffort":"high","supportedReasoningEfforts":[
                    {"reasoningEffort":"high","description":"Deep"}
                 ]}
            ],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    assert!(runtime.session_config().is_none());
}

#[test]
fn missing_reported_effort_never_becomes_catalog_default_effort() {
    for resume in [false, true] {
        let mut runtime = fixture_runtime();
        initialize(&mut runtime);
        let model = Some(RequiredText::new("modelId", "actual").unwrap());
        let command = if resume {
            AdapterCommand::ResumeSession {
                external_session_id: ExternalId::new("thread", "thread-1").unwrap(),
                native_root: r"C:\repo".into(),
                model,
            }
        } else {
            AdapterCommand::NewSession {
                native_root: r"C:\repo".into(),
                model,
            }
        };
        let start = runtime.command(command).unwrap();
        runtime
            .ingest(
                &frame(json!({"id":request_id(&start),"result":{
                    "thread":{"id":"thread-1"},"model":"actual"
                }})),
                &Mapper,
            )
            .unwrap();
        let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
        runtime
            .ingest(
                &frame(json!({"id":list_id,"result":{"data":[
                    {"id":"actual","displayName":"Actual model","isDefault":true,
                     "defaultReasoningEffort":"high","supportedReasoningEfforts":[
                        {"reasoningEffort":"high","description":"Deep"}
                     ]}
                ],"nextCursor":null}})),
                &Mapper,
            )
            .unwrap();
        let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
        assert_eq!(
            config["confirmedModel"],
            json!({"id":"actual","name":"Actual model"})
        );
        assert_eq!(config["configOptions"], json!([]));
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "continue").unwrap(),
                client_message_id: RequiredText::new("client", "no-inferred-effort").unwrap(),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .unwrap();
        let request: Value = serde_json::from_slice(prompt.frame().payload()).unwrap();
        assert!(request["params"].get("effort").is_none());
    }
}

#[test]
fn requested_model_without_reported_identity_is_not_confirmed_by_the_catalog() {
    for resume in [false, true] {
        let mut runtime = fixture_runtime();
        initialize(&mut runtime);
        let model = Some(RequiredText::new("modelId", "requested").unwrap());
        let command = if resume {
            AdapterCommand::ResumeSession {
                external_session_id: ExternalId::new("thread", "thread-1").unwrap(),
                native_root: r"C:\repo".into(),
                model,
            }
        } else {
            AdapterCommand::NewSession {
                native_root: r"C:\repo".into(),
                model,
            }
        };
        let start = runtime.command(command).unwrap();
        runtime
            .ingest(
                &frame(json!({"id":request_id(&start),"result":{
                    "thread":{"id":"thread-1"},"reasoningEffort":"high"
                }})),
                &Mapper,
            )
            .unwrap();
        assert!(runtime.session_config().is_none());
        let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
        runtime
            .ingest(
                &frame(json!({"id":list_id,"result":{"data":[
                    {"id":"requested","displayName":"Requested model","isDefault":true,
                     "defaultReasoningEffort":"high","supportedReasoningEfforts":[
                        {"reasoningEffort":"high","description":"Deep"}
                     ]}
                ],"nextCursor":null}})),
                &Mapper,
            )
            .unwrap();
        assert!(runtime.session_config().is_none());
    }
}
