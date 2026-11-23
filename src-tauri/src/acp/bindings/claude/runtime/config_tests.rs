use super::tests::{Mapper, frame, initialize, request_id};
use super::*;
use sage_acp_protocol::{
    contract::{SageChatId, SageProjectId},
    scheduler::RuntimeOwner,
    session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigValue},
};

fn ready_config(resume: bool) -> ClaudeRuntime {
    ready_config_options(
        resume,
        json!([{"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"low","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}]),
    )
}
fn ready_config_options(resume: bool, options: Value) -> ClaudeRuntime {
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
    runtime.ingest(&frame(json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":"external-a","modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask"},{"id":"code","name":"Code"}]},"configOptions":options}})), &Mapper).unwrap();
    runtime
}
#[test]
fn stray_defaults_never_enable_claude_reset_or_reject_session_configuration() {
    for resume in [false, true] {
        for default in [
            json!("high"),
            json!("unsupported"),
            json!(false),
            json!(null),
            json!({"bad":1}),
        ] {
            let runtime = ready_config_options(
                resume,
                json!([
                    {"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"low","defaultValue":default,"options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]},
                    {"id":"flag","name":"Flag","type":"boolean","currentValue":true,"defaultValue":default}
                ]),
            );
            let wire = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
            assert_eq!(wire["configOptions"][0]["currentValue"], "low");
            assert_eq!(wire["configOptions"][1]["currentValue"], true);
            for option in wire["configOptions"].as_array().unwrap() {
                assert!(option.get("defaultValue").is_none());
            }
        }
    }
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
fn config_notifications_republish_control_placement_only_when_it_changes() {
    use crate::acp::{bindings::runtime::NativeRuntime, slash_commands::CommandChannel};
    use std::sync::atomic::{AtomicU64, Ordering};
    let claude = ready_config(false);
    let effort =
        serde_json::to_value(claude.session_config().unwrap()).unwrap()["configOptions"].clone();
    let owner = intent(ConfigMutation::Mode(ConfigId::new("mode", "ask").unwrap())).owner;
    let channel = Arc::new(CommandChannel::default());
    let announced = Arc::new(AtomicU64::new(0));
    let sink = announced.clone();
    channel.set_sink(Arc::new(move |notice| {
        sink.store(notice.revision, Ordering::SeqCst);
    }));
    channel.reserve(owner.clone());
    let mut runtime = NativeRuntime::new(claude);
    runtime.attach_commands(owner, channel);
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
        "sessionId":"external-a","update":{"sessionUpdate":"available_commands_update",
        "availableCommands":[{"name":"effort","description":"Adjust effort"}]}}})),
            &Mapper,
        )
        .unwrap();
    let mut revision = runtime.available_commands().snapshot().revision;
    for (options, control, changed) in [
        (json!([]), None, true),
        (effort.clone(), Some("effortPicker"), true),
        (effort, Some("effortPicker"), false),
    ] {
        runtime.ingest(&frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId":"external-a","update":{"sessionUpdate":"config_option_update","configOptions":options}}})), &Mapper).unwrap();
        let list = runtime.available_commands().snapshot();
        revision += u64::from(changed);
        assert_eq!(list.revision, revision);
        assert_eq!(announced.load(Ordering::SeqCst), revision);
        let placement = serde_json::to_value(&list.commands[0]).unwrap()["placement"].clone();
        assert_eq!(placement["section"], "modelAndAccess");
        assert_eq!(placement["control"].as_str(), control);
        assert!(list.validate(revision, "/effort").is_ok());
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
        let wire = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
        assert_eq!(wire["configOptions"][0]["category"], "thought_level");
        assert_eq!(wire["configOptions"][0]["options"][0]["value"], "low");
        assert_eq!(wire["configOptions"][0]["options"][1]["value"], "high");
        assert!(wire["configOptions"][0].get("defaultValue").is_none());
    }
}
#[test]
fn idle_mode_update_changes_state_but_set_mode_ack_never_echoes() {
    let mut runtime = ready_config(false);
    let egress = runtime
        .configure(&intent(ConfigMutation::Mode(
            ConfigId::new("mode", "code").unwrap(),
        )))
        .unwrap()
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
    let egress = runtime.configure(&mutation("high")).unwrap().unwrap();
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
        .unwrap()
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
        .unwrap()
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
        .unwrap()
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
            .unwrap()
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

#[test]
fn a_harness_deriving_nothing_from_its_catalog_never_restores_capabilities() {
    use crate::acp::{bindings::runtime::NativeRuntime, runtime::ProjectionMailbox};
    let (database, fixture) = crate::acp::store::title_fixture();
    let read = || -> (String, i64) {
        database
            .connection()
            .query_row(
                "SELECT capabilities_json,projection_revision FROM agent_sessions WHERE id=?1",
                [fixture.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    };
    let before = read();
    let claude = ready_config(false);
    let ceiling = claude.capabilities().unwrap();
    let notices = Arc::new(ProjectionMailbox::default());
    let mut runtime = NativeRuntime::new(claude);
    runtime.attach_catalog_capabilities(
        crate::acp::service::catalog_capabilities::CatalogCapabilities::new(
            RuntimeOwner {
                project_id: fixture.project_id.clone(),
                chat_id: fixture.chat_id.clone(),
                session_id: fixture.session_id.clone(),
                runtime_epoch: fixture.runtime_epoch,
            },
            (ceiling.clone(), ceiling),
            database.clone(),
            notices.clone(),
        ),
    );
    for commands in [json!([{"name":"goal"}]), json!([])] {
        runtime
            .ingest(
                &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId":"external-a","update":{"sessionUpdate":"available_commands_update",
            "availableCommands":commands}}})),
                &Mapper,
            )
            .unwrap();
    }
    assert_eq!(read(), before);
    assert!(notices.take().is_none());
}
