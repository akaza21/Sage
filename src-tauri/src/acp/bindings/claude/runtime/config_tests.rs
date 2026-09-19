use super::tests::{Mapper, frame, initialize, request_id};
use super::*;
use crate::acp::{
    contract::{SageChatId, SageProjectId},
    scheduler::RuntimeOwner,
    session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigValue},
};

fn ready_config(resume: bool) -> ClaudeRuntime {
    let mut runtime = ClaudeRuntime::fixture();
    initialize(&mut runtime);
    let command = if resume {
        AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("session", "external-a").unwrap(),
            native_root: "C:\\fixture".into(),
            model: None,
        }
    } else {
        AdapterCommand::NewSession {
            native_root: "C:\\fixture".into(),
            model: None,
        }
    };
    let egress = runtime.command(command).unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    runtime.ingest(&frame(json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":"external-a","modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask"},{"id":"code","name":"Code"}]},"configOptions":[{"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"low","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}]}})), &Mapper).unwrap();
    runtime
}
fn intent(mutation: ConfigMutation) -> ConfigIntent {
    ConfigIntent {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-a").unwrap(),
            chat_id: SageChatId::parse("chat-a").unwrap(),
            session_id: SageAgentSessionId::parse("session-claude-runtime").unwrap(),
            runtime_epoch: RuntimeEpoch::new(3).unwrap(),
        },
        expected_config_revision: serde_json::from_value(json!("1")).unwrap(),
        mutation,
    }
}
#[test]
fn new_and_resume_preserve_configuration_without_durable_capability_count() {
    for resume in [false, true] {
        let runtime = ready_config(resume);
        assert_eq!(
            runtime
                .session_config()
                .unwrap()
                .config_options
                .as_slice()
                .len(),
            1
        );
        assert_eq!(runtime.capabilities().unwrap().config_count, 0);
    }
}
#[test]
fn idle_mode_update_changes_state_but_set_mode_ack_never_echoes() {
    let mut runtime = ready_config(false);
    let egress = runtime
        .configure(&intent(ConfigMutation::Mode(
            ConfigId::new("mode", "code").unwrap(),
        )))
        .unwrap();
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(request["method"], "session/set_mode");
    assert_eq!(
        request["params"],
        json!({"sessionId":"external-a","modeId":"code"})
    );
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{"_meta":{}}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        runtime
            .session_config()
            .unwrap()
            .modes
            .unwrap()
            .current_mode_id
            .as_str(),
        "ask"
    );
    runtime.ingest(&frame(json!({"method":"session/update","params":{"sessionId":"external-a","update":{"sessionUpdate":"current_mode_update","currentModeId":"code"}}})), &Mapper).unwrap();
    assert_eq!(
        runtime
            .session_config()
            .unwrap()
            .modes
            .unwrap()
            .current_mode_id
            .as_str(),
        "code"
    );
}
#[test]
fn config_request_validates_and_complete_response_replaces_atomically() {
    let mut runtime = ready_config(false);
    let mutation = |value| {
        intent(ConfigMutation::Option {
            config_id: ConfigId::new("id", "effort").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", value).unwrap(),
            },
        })
    };
    assert!(runtime.configure(&mutation("missing")).is_err());
    let egress = runtime.configure(&mutation("high")).unwrap();
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(
        request["params"],
        json!({"sessionId":"external-a","configId":"effort","value":"high"})
    );
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    runtime.ingest(&frame(json!({"jsonrpc":"2.0","id":id,"result":{"configOptions":[{"id":"flag","name":"Flag","type":"boolean","currentValue":false}]}})), &Mapper).unwrap();
    let state = runtime.session_config().unwrap();
    assert_eq!(state.config_options.as_slice()[0].id.as_str(), "flag");
    let before = state.clone();
    let boolean = runtime
        .configure(&intent(ConfigMutation::Option {
            config_id: ConfigId::new("id", "flag").unwrap(),
            value: ConfigValue::Boolean { value: true },
        }))
        .unwrap();
    let boolean_wire: Value = serde_json::from_slice(boolean.frame().payload()).unwrap();
    assert_eq!(
        boolean_wire["params"],
        json!({"sessionId":"external-a","configId":"flag","type":"boolean","value":true})
    );
    runtime
        .resolve_egress(boolean.into_token(), EgressResolution::WriteZeroBytes)
        .unwrap();
    assert!(runtime.ingest(&frame(json!({"method":"session/update","params":{"sessionId":"external-a","update":{"sessionUpdate":"config_option_update","configOptions":[{"id":"bad"}]}}})), &Mapper).is_err());
    assert_eq!(runtime.session_config().unwrap(), before);
}
#[test]
fn rejected_config_mutations_are_typed_nonfatal_and_preserve_authority() {
    let mut mode_runtime = ready_config(false);
    let before_mode = mode_runtime.session_config().unwrap();
    let mode = mode_runtime
        .configure(&intent(ConfigMutation::Mode(
            ConfigId::new("mode", "code").unwrap(),
        )))
        .unwrap();
    let mode_id = request_id(&mode);
    mode_runtime
        .resolve_egress(mode.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(matches!(
        mode_runtime.ingest(
            &frame(json!({
                "jsonrpc":"2.0", "id":mode_id,
                "error":{"code":-32000,"message":"mode refused"}
            })),
            &Mapper,
        ),
        Err(AdapterRuntimeError::ConfigMutationRejected)
    ));
    assert!(!mode_runtime.config_pending());
    assert_eq!(mode_runtime.session_config().unwrap(), before_mode);
    assert!(
        mode_runtime
            .configure(&intent(ConfigMutation::Mode(
                ConfigId::new("mode", "code").unwrap(),
            )))
            .is_ok(),
        "a protocol-valid rejection must leave the adapter reusable"
    );

    let mut option_runtime = ready_config(false);
    let before_option = option_runtime.session_config().unwrap();
    let option = option_runtime
        .configure(&intent(ConfigMutation::Option {
            config_id: ConfigId::new("id", "effort").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", "high").unwrap(),
            },
        }))
        .unwrap();
    let option_id = request_id(&option);
    option_runtime
        .resolve_egress(option.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(matches!(
        option_runtime.ingest(
            &frame(json!({
                "jsonrpc":"2.0", "id":option_id,
                "error":{"code":-32001,"message":"option refused"}
            })),
            &Mapper,
        ),
        Err(AdapterRuntimeError::ConfigMutationRejected)
    ));
    assert!(!option_runtime.config_pending());
    assert_eq!(option_runtime.session_config().unwrap(), before_option);
}

#[test]
fn malformed_config_error_response_remains_fatal() {
    for error in [json!(null), json!(true), json!({"code":-32000})] {
        let mut runtime = ready_config(false);
        let egress = runtime
            .configure(&intent(ConfigMutation::Mode(
                ConfigId::new("mode", "code").unwrap(),
            )))
            .unwrap();
        let id = request_id(&egress);
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .unwrap();
        assert!(matches!(
            runtime.ingest(
                &frame(json!({"jsonrpc":"2.0","id":id,"error":error})),
                &Mapper,
            ),
            Err(AdapterRuntimeError::Frame)
        ));
    }
}

#[test]
fn config_wire_rejects_duplicate_decoded_keys() {
    assert!(decode_frame(br#"{"method":"session/update","params":{"sessionId":"external-a","update":{"sessionUpdate":"config_option_update","configOptions":[],"configOptions":[]}}}"#).is_err());
}

#[test]
fn config_updates_are_exact_session_and_boolean_support_is_advertised() {
    let mut runtime = ClaudeRuntime::fixture();
    let initialize = runtime.command(AdapterCommand::Initialize).unwrap();
    let wire: Value = serde_json::from_slice(initialize.frame().payload()).unwrap();
    assert_eq!(
        wire["params"]["clientCapabilities"]["session"]["configOptions"]["boolean"],
        json!({})
    );
    let mut runtime = ready_config(false);
    assert!(runtime.ingest(&frame(json!({"method":"session/update","params":{"sessionId":"foreign","update":{"sessionUpdate":"config_option_update","configOptions":[]}}})), &Mapper).is_err());
    runtime.ingest(&frame(json!({"method":"session/update","params":{"sessionId":"external-a","update":{"sessionUpdate":"config_option_update","configOptions":[]}}})), &Mapper).unwrap();
    assert!(
        runtime
            .session_config()
            .unwrap()
            .config_options
            .as_slice()
            .is_empty()
    );
}
