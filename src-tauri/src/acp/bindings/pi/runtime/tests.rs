use serde_json::{Value, json};

mod config_delivery;
mod followups;
mod pump_contract;
mod regressions;
mod store_ingress;

use super::*;
use crate::acp::{
    bindings::{
        adapter::{
            AdapterCommand, AdapterEvent, AdapterRuntimeError, EgressResolution, HarnessAdapter,
        },
        pi::{
            ArtifactCandidate, ArtifactIdentity, CuratedPiBinding, PI_LICENSE, PI_NODE_MINIMUM,
            PI_PACKAGE, PI_RPC_ARGS, PI_SOURCE_COMMIT, PI_VERSION, PiPlatform,
        },
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SageRootId, SageServerRequestId},
    intent::{
        ExternalId, NormalizedPayload, Prompt, SafeResourceLocation, SafeResourceRef, ToolCategory,
    },
    transport::IncrementalFrameDecoder,
};

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        path: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>
    {
        if path == r"C:\repo\src\lib.rs" {
            return SafeResourceRef::new(SageRootId::parse("root-pi").unwrap(), "src/lib.rs")
                .map_err(|_| crate::acp::intent::UnavailableResourceReason::Unsupported);
        }
        Err(crate::acp::intent::UnavailableResourceReason::Unsupported)
    }
}

fn fixture_runtime(epoch: u64) -> PiRuntime {
    PiRuntime::fixture(
        RuntimeEpoch::new(epoch).expect("epoch"),
        SageAgentSessionId::parse(format!("session-pi-{epoch}")).expect("session"),
    )
}

fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).expect("json");
    bytes.push(b'\n');
    let mut decoder = IncrementalFrameDecoder::new();
    let mut frame = None;
    for chunk in bytes.chunks(8_192) {
        let next = decoder.push_chunk(chunk).expect("valid frame");
        assert!(frame.is_none());
        frame = next.or(frame);
    }
    frame.expect("complete frame")
}

fn request(egress: &AdapterEgress) -> Value {
    serde_json::from_slice(egress.frame().payload()).expect("request json")
}

fn deliver(runtime: &mut PiRuntime, egress: AdapterEgress) -> Value {
    let value = request(&egress);
    let token = egress.into_token();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    value
}

fn response(request: &Value, data: Value) -> Frame {
    frame(json!({
        "type":"response",
        "id":request["id"],
        "command":request["type"],
        "success":true,
        "data":data
    }))
}

fn initialize(runtime: &mut PiRuntime) {
    let state = runtime
        .command(AdapterCommand::Initialize)
        .expect("initialize");
    assert_eq!(request(&state)["type"], "get_state");
    let state = deliver(runtime, state);
    let batch = runtime
        .ingest(
            &response(
                &state,
                json!({"model":{"provider":"openai","id":"gpt","name":"GPT"},"thinkingLevel":"high"}),
            ),
            &Mapper,
        )
        .expect("state response");
    assert!(batch.events().is_empty());
    assert_eq!(batch.egress().len(), 1);
    let models = request(&batch.egress()[0]);
    let token = batch.egress()[0].token().clone();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("models queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("models written");
    let batch = runtime
        .ingest(
            &response(
                &models,
                json!({"models":[{"provider":"openai","id":"gpt","name":"GPT"}]}),
            ),
            &Mapper,
        )
        .expect("models response");
    assert!(matches!(
        batch.events(),
        [
            AdapterEvent::Models { .. },
            AdapterEvent::Initialized { .. }
        ]
    ));
    let AdapterEvent::Models { models, .. } = &batch.events()[0] else {
        panic!("models")
    };
    assert_eq!(models.iter().filter(|model| model.is_default).count(), 1);
}

fn ready(runtime: &mut PiRuntime) {
    let levels = ready_until_levels(runtime);
    let batch = runtime
        .ingest(
            &response(&levels, json!({"levels":["off","high"]})),
            &Mapper,
        )
        .expect("levels response");
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredDuplicate]));
    assert!(batch.egress().is_empty());
}

fn ready_until_levels(runtime: &mut PiRuntime) -> Value {
    initialize(runtime);
    let new_session = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .expect("new session");
    let value = deliver(runtime, new_session);
    let batch = runtime
        .ingest(&response(&value, Value::Null), &Mapper)
        .expect("session response");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::SessionReady { .. }]
    ));
    assert!(batch.egress().is_empty());
    let levels = runtime
        .begin_thinking_discovery()
        .expect("post-activation thinking discovery");
    let levels = deliver(runtime, levels);
    assert_eq!(levels["type"], "get_available_thinking_levels");
    levels
}

fn running(runtime: &mut PiRuntime) {
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect read-only").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-pi-1").expect("client"),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .expect("prompt");
    let value = deliver(runtime, prompt);
    runtime
        .ingest(&response(&value, Value::Null), &Mapper)
        .expect("prompt response");
    let batch = runtime
        .ingest(&frame(json!({"type":"agent_start"})), &Mapper)
        .expect("agent start");
    assert!(matches!(batch.events(), [AdapterEvent::Turn { .. }]));
}

#[test]
fn pi_thinking_delta_projects_thought_summary_not_assistant_text() {
    let mut runtime = fixture_runtime(31);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(&frame(json!({"type":"message_start"})), &Mapper)
        .expect("message start");
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"message_update",
                "assistantMessageEvent":{"type":"thinking_delta","contentIndex":0,"delta":"Check the fixture"}
            })),
            &Mapper,
        )
        .expect("thinking delta");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ThoughtSummary { summary },
            ..
        }] if summary.as_str() == "Check the fixture"
    ));
}

#[test]
fn pi_read_tool_projects_category_and_project_location() {
    let mut runtime = fixture_runtime(32);
    ready(&mut runtime);
    running(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"tool_execution_start",
                "toolCallId":"read-file",
                "toolName":"read",
                "args":{"path":r"C:\repo\src\lib.rs"}
            })),
            &Mapper,
        )
        .expect("read tool");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { category: ToolCategory::Read, locations, .. },
            ..
        }] if matches!(locations.as_slice(), [SafeResourceLocation::Resource { resource }]
            if resource.relative_path.as_str() == "src/lib.rs")
    ));
}

#[test]
fn pi_read_tool_projects_bounded_result_without_losing_identity() {
    let mut runtime = fixture_runtime(33);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(
            &frame(json!({
                "type":"tool_execution_start",
                "toolCallId":"read-result",
                "toolName":"read",
                "args":{"path":r"C:\repo\src\lib.rs"}
            })),
            &Mapper,
        )
        .expect("read start");
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"tool_execution_update",
                "toolCallId":"read-result",
                "partialResult":{"content":[{"type":"text","text":"first line"}]}
            })),
            &Mapper,
        )
        .expect("read update");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { category: ToolCategory::Read, output_summary: Some(summary), .. },
            ..
        }] if summary.as_str() == "first line"
    ));
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"tool_execution_update",
                "toolCallId":"read-result",
                "partialResult":{"content":[]}
            })),
            &Mapper,
        )
        .expect("empty replacement snapshot");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall {
                output_summary: None,
                ..
            },
            ..
        }]
    ));
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"tool_execution_end",
                "toolCallId":"read-result",
                "result":{"content":[{"type":"text","text":"final line"}]},
                "isError":false
            })),
            &Mapper,
        )
        .expect("read end");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { category: ToolCategory::Read, output_summary: Some(summary), .. },
            ..
        }] if summary.as_str() == "final line"
    ));
}

#[test]
fn pi_valid_and_invalid_read_images_are_visible_unsupported_without_bytes() {
    for (epoch, data) in [
        (
            34,
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/xioAAAAASUVORK5CYII=",
        ),
        (35, "not-base64"),
    ] {
        let mut runtime = fixture_runtime(epoch);
        ready(&mut runtime);
        running(&mut runtime);
        runtime
            .ingest(
                &frame(json!({
                    "type":"tool_execution_start",
                    "toolCallId":"read-image",
                    "toolName":"read",
                    "args":{"path":r"C:\repo\src\lib.rs"}
                })),
                &Mapper,
            )
            .expect("read start");
        let batch = runtime
            .ingest(
                &frame(json!({
                    "type":"tool_execution_end",
                    "toolCallId":"read-image",
                    "result":{"content":[{"type":"image","mimeType":"image/png","data":data}]},
                    "isError":false
                })),
                &Mapper,
            )
            .expect("image fallback remains nonfatal");
        assert!(batch.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Item {
                lifecycle: crate::acp::lifecycle::ItemLifecycle::Unsupported,
                payload: NormalizedPayload::Unsupported { external_kind, summary },
                ..
            } if external_kind.as_str() == "pi_image"
                && summary.as_str() == "Image output is unavailable: shared durable image admission is missing"
        )));
        assert!(!batch.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Item {
                payload: NormalizedPayload::Image { .. },
                ..
            }
        )));
        let projected = serde_json::to_string(batch.events()).expect("events json");
        assert!(!projected.contains(data));
    }
}

#[test]
fn pi_tool_snapshot_cache_rejects_aggregate_overflow() {
    let mut runtime = fixture_runtime(46);
    ready(&mut runtime);
    running(&mut runtime);
    let mut overflow = None;
    for index in 0..129 {
        let id = format!("read-{index}");
        runtime
            .ingest(
                &frame(json!({"type":"tool_execution_start","toolCallId":id,"toolName":"read"})),
                &Mapper,
            )
            .expect("read start");
        let result = runtime.ingest(
            &frame(json!({
                "type":"tool_execution_update",
                "toolCallId":id,
                "partialResult":{"content":[{"type":"text","text":"x".repeat(65_536)}]}
            })),
            &Mapper,
        );
        if let Err(error) = result {
            overflow = Some(error);
            break;
        }
    }
    assert!(matches!(overflow, Some(AdapterRuntimeError::BatchBytes)));
    assert!(matches!(
        runtime.command(AdapterCommand::Cancel),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn pi_unproven_stream_shapes_and_features_remain_unsupported() {
    let mut runtime = fixture_runtime(47);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(&frame(json!({"type":"message_start"})), &Mapper)
        .expect("message start");
    for event in [
        json!({"type":"message_update","delta":"untyped arguments"}),
        json!({"type":"plan","entries":[{"content":"not a Pi plan event","status":"pending"}]}),
        json!({"type":"subagent_start","id":"unproven-child"}),
    ] {
        let batch = runtime
            .ingest(&frame(event), &Mapper)
            .expect("unsupported event");
        assert!(matches!(batch.events(), [AdapterEvent::Unsupported(_)]));
        assert!(batch.egress().is_empty());
    }
}

#[test]
fn pi_toolcall_stream_arguments_are_absorbed_without_items() {
    let mut runtime = fixture_runtime(47);
    ready(&mut runtime);
    running(&mut runtime);
    let batch = runtime.ingest(&frame(json!({"type":"message_update","assistantMessageEvent":{"type":"toolcall_delta","delta":"arguments"}})), &Mapper).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Absorbed]));
    assert!(batch.egress().is_empty());
}

#[test]
fn pi_tool_snapshot_budget_is_reused_after_replacement_and_completion() {
    let mut runtime = fixture_runtime(49);
    ready(&mut runtime);
    running(&mut runtime);
    for index in 0..129 {
        let id = format!("read-{index}");
        runtime
            .ingest(
                &frame(json!({"type":"tool_execution_start","toolCallId":id,"toolName":"read"})),
                &Mapper,
            )
            .expect("terminal snapshots leave room for the next read");
        for text in [
            "x".repeat(65_536),
            "y".repeat(65_536),
            "small".into(),
            "z".repeat(65_536),
        ] {
            runtime
                .ingest(
                    &frame(json!({
                        "type":"tool_execution_update",
                        "toolCallId":id,
                        "partialResult":{"content":[{"type":"text","text":text}]}
                    })),
                    &Mapper,
                )
                .expect("snapshots replace rather than accumulate byte reservations");
        }
        let batch = runtime
            .ingest(
                &frame(json!({
                    "type":"tool_execution_end",
                    "toolCallId":id,
                    "result":{"content":[{"type":"text","text":"final".repeat(13_107)}]},
                    "isError":false
                })),
                &Mapper,
            )
            .expect("tool completion releases its retained snapshot");
        assert!(matches!(
            batch.events(),
            [AdapterEvent::Item {
                lifecycle: crate::acp::lifecycle::ItemLifecycle::Completed,
                ..
            }]
        ));
    }
}

#[test]
fn pi_available_models_project_session_config() {
    let mut runtime = fixture_runtime(36);
    initialize(&mut runtime);
    let config = runtime.session_config().expect("Pi config");
    let wire = serde_json::to_value(config).expect("config json");
    assert_eq!(wire["configOptions"][0]["id"], "pi.model");
    assert_eq!(wire["configOptions"][0]["currentValue"], "openai:gpt");
    assert_eq!(
        wire["configOptions"][0]["options"][0]["value"],
        "openai:gpt"
    );
}

#[test]
fn pi_model_selection_uses_set_model_and_updates_config_after_reply() {
    use crate::acp::{
        contract::{SageChatId, SageProjectId},
        scheduler::RuntimeOwner,
        session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigValue},
    };

    let mut runtime = fixture_runtime(37);
    ready(&mut runtime);
    let models = runtime
        .command(AdapterCommand::ModelPage { cursor: None })
        .expect("models page");
    let models = deliver(&mut runtime, models);
    runtime
        .ingest(
            &response(
                &models,
                json!({"models":[
                    {"provider":"openai","id":"gpt","name":"GPT"},
                    {"provider":"anthropic","id":"claude","name":"Claude"}
                ]}),
            ),
            &Mapper,
        )
        .expect("models reply");
    let intent = ConfigIntent {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-pi").unwrap(),
            chat_id: SageChatId::parse("chat-pi").unwrap(),
            session_id: SageAgentSessionId::parse("session-pi-37").unwrap(),
            runtime_epoch: RuntimeEpoch::new(37).unwrap(),
        },
        expected_config_revision: serde_json::from_value(json!("1")).unwrap(),
        mutation: ConfigMutation::Option {
            config_id: ConfigId::new("configId", "pi.model").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", "anthropic:claude").unwrap(),
            },
        },
    };
    let egress = runtime.configure(&intent).expect("model selection");
    let model_request = deliver(&mut runtime, egress);
    assert_eq!(model_request["type"], "set_model");
    assert_eq!(model_request["provider"], "anthropic");
    assert_eq!(model_request["modelId"], "claude");
    let batch = runtime
        .ingest(
            &response(
                &model_request,
                json!({"provider":"anthropic","id":"claude","name":"Claude"}),
            ),
            &Mapper,
        )
        .expect("model selected");
    let wire = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(wire["configOptions"][0]["currentValue"], "anthropic:claude");
    assert!(wire["configOptions"][1].is_null());
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredDuplicate]));
    assert!(batch.egress().is_empty());
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect").unwrap(),
            client_message_id: RequiredText::new("client", "model-refresh").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt = deliver(&mut runtime, prompt);
    let batch = runtime
        .ingest(&response(&prompt, Value::Null), &Mapper)
        .unwrap();
    assert_eq!(batch.egress().len(), 1);
    let state = request(&batch.egress()[0]);
    let token = batch.egress()[0].token().clone();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .unwrap();
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    assert_eq!(state["type"], "get_state");
    let batch = runtime
        .ingest(
            &response(
                &state,
                json!({"model":{"provider":"anthropic","id":"claude"},"thinkingLevel":"off"}),
            ),
            &Mapper,
        )
        .expect("refreshed state");
    assert_eq!(batch.egress().len(), 1);
    let levels = request(&batch.egress()[0]);
    let token = batch.egress()[0].token().clone();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .unwrap();
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(&response(&levels, json!({"levels":["off"]})), &Mapper)
        .expect("new model levels");
    let wire = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(wire["configOptions"][1]["currentValue"], "off");
}

#[test]
fn pi_thinking_config_uses_reported_levels_only() {
    let mut runtime = fixture_runtime(38);
    ready(&mut runtime);
    let wire = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(wire["configOptions"][1]["id"], "pi.thinking");
    assert_eq!(wire["configOptions"][1]["currentValue"], "high");
    assert_eq!(wire["configOptions"][1]["options"][0]["value"], "off");
    assert_eq!(wire["configOptions"][1]["options"][1]["value"], "high");
    assert!(wire["configOptions"][1]["options"][2].is_null());
}

#[test]
fn pi_thinking_selection_is_correlated_and_model_bounded() {
    use crate::acp::{
        contract::{SageChatId, SageProjectId},
        scheduler::RuntimeOwner,
        session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigValue},
    };

    let mut runtime = fixture_runtime(39);
    ready(&mut runtime);
    let owner = RuntimeOwner {
        project_id: SageProjectId::parse("project-pi").unwrap(),
        chat_id: SageChatId::parse("chat-pi").unwrap(),
        session_id: SageAgentSessionId::parse("session-pi-39").unwrap(),
        runtime_epoch: RuntimeEpoch::new(39).unwrap(),
    };
    let intent = |level: &str| ConfigIntent {
        owner: owner.clone(),
        expected_config_revision: serde_json::from_value(json!("1")).unwrap(),
        mutation: ConfigMutation::Option {
            config_id: ConfigId::new("configId", "pi.thinking").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", level).unwrap(),
            },
        },
    };
    assert!(matches!(
        runtime.configure(&intent("max")),
        Err(AdapterRuntimeError::Operation)
    ));
    let egress = runtime
        .configure(&intent("off"))
        .expect("thinking selection");
    let request = deliver(&mut runtime, egress);
    assert_eq!(request["type"], "set_thinking_level");
    assert_eq!(request["level"], "off");
    assert!(runtime.config_pending());
    let before = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(before["configOptions"][1]["currentValue"], "high");
    runtime
        .ingest(&response(&request, Value::Null), &Mapper)
        .expect("thinking selected");
    assert!(!runtime.config_pending());
    let after = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(after["configOptions"][1]["currentValue"], "off");
}

#[test]
fn pi_poisoned_idle_runtime_rejects_config_and_thinking_discovery() {
    use crate::acp::{
        contract::{SageChatId, SageProjectId},
        scheduler::RuntimeOwner,
        session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigValue},
    };

    let mut runtime = fixture_runtime(48);
    ready(&mut runtime);
    let models = runtime
        .command(AdapterCommand::ModelPage { cursor: None })
        .expect("models request");
    runtime
        .resolve_egress(models.into_token(), EgressResolution::DeliveryUnknown)
        .expect("unknown delivery poisons runtime");
    let intent = ConfigIntent {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-pi").unwrap(),
            chat_id: SageChatId::parse("chat-pi").unwrap(),
            session_id: SageAgentSessionId::parse("session-pi-48").unwrap(),
            runtime_epoch: RuntimeEpoch::new(48).unwrap(),
        },
        expected_config_revision: serde_json::from_value(json!("1")).unwrap(),
        mutation: ConfigMutation::Option {
            config_id: ConfigId::new("configId", "pi.model").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", "openai:gpt").unwrap(),
            },
        },
    };
    assert!(matches!(
        runtime.configure(&intent),
        Err(AdapterRuntimeError::Operation)
    ));
    assert!(matches!(
        runtime.begin_thinking_discovery(),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn pi_late_levels_add_thinking_option_after_session_ready() {
    let mut runtime = fixture_runtime(40);
    let levels = ready_until_levels(&mut runtime);
    let before = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(before["configOptions"][0]["id"], "pi.model");
    assert!(before["configOptions"][1].is_null());
    assert_eq!(runtime.capabilities().unwrap().config_count, 1);
    runtime
        .ingest(
            &response(&levels, json!({"levels":["off","high"]})),
            &Mapper,
        )
        .expect("levels reply");
    let after = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(after["configOptions"][1]["id"], "pi.thinking");
    assert_eq!(runtime.capabilities().unwrap().config_count, 2);
}

#[test]
fn pi_empty_thinking_levels_keep_model_only_config() {
    let mut runtime = fixture_runtime(41);
    let levels = ready_until_levels(&mut runtime);
    runtime
        .ingest(&response(&levels, json!({"levels":[]})), &Mapper)
        .expect("empty levels");
    let wire = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert!(wire["configOptions"][1].is_null());
    assert_eq!(runtime.capabilities().unwrap().config_count, 1);
}

#[test]
fn pi_malformed_or_failed_idle_levels_keep_model_only_without_egress() {
    let mut malformed = fixture_runtime(42);
    let levels = ready_until_levels(&mut malformed);
    let batch = malformed
        .ingest(&response(&levels, json!({"levels":"bad"})), &Mapper)
        .expect("malformed levels diagnostic");
    assert!(
        batch
            .events()
            .iter()
            .any(|event| matches!(event, AdapterEvent::IgnoredDuplicate))
    );
    assert!(
        serde_json::to_value(malformed.session_config().unwrap()).unwrap()["configOptions"][1]
            .is_null()
    );

    let mut failed = fixture_runtime(43);
    let levels = ready_until_levels(&mut failed);
    let batch = failed
        .ingest(&frame(json!({"type":"response","id":levels["id"],"command":"get_available_thinking_levels","success":false})), &Mapper)
        .expect("failed levels diagnostic");
    assert!(
        batch
            .events()
            .iter()
            .any(|event| matches!(event, AdapterEvent::IgnoredDuplicate))
    );
    assert!(
        serde_json::to_value(failed.session_config().unwrap()).unwrap()["configOptions"][1]
            .is_null()
    );
}

#[test]
fn pi_levels_for_ended_session_or_other_owner_are_dropped() {
    let mut runtime = fixture_runtime(44);
    let levels = ready_until_levels(&mut runtime);
    runtime.external_session = Some(ExternalId::new("session", "replacement").unwrap());
    let batch = runtime
        .ingest(
            &response(&levels, json!({"levels":["off","high"]})),
            &Mapper,
        )
        .expect("late levels");
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert!(
        serde_json::to_value(runtime.session_config().unwrap()).unwrap()["configOptions"][1]
            .is_null()
    );

    let mut other = fixture_runtime(45);
    let levels = ready_until_levels(&mut other);
    other.session_id = SageAgentSessionId::parse("other-owner").unwrap();
    let batch = other
        .ingest(
            &response(&levels, json!({"levels":["off","high"]})),
            &Mapper,
        )
        .expect("foreign owner levels");
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert!(
        serde_json::to_value(other.session_config().unwrap()).unwrap()["configOptions"][1]
            .is_null()
    );
}

fn identity(path: &str, byte: char) -> ArtifactIdentity {
    ArtifactIdentity {
        relative_path: path.into(),
        sha256: byte.to_string().repeat(64),
        bytes: 42,
    }
}

#[test]
fn exact_candidate_remains_launch_unavailable() {
    let entrypoint = identity("pi/dist/cli.js", 'a');
    let node = identity("node/node.exe", 'b');
    let curated = CuratedPiBinding {
        platform: PiPlatform::WindowsX64,
        entrypoint: entrypoint.clone(),
        runtime: node.clone(),
        node_version: PI_NODE_MINIMUM.into(),
    };
    let candidate = ArtifactCandidate {
        platform: PiPlatform::WindowsX64,
        package_name: PI_PACKAGE.into(),
        package_version: PI_VERSION.into(),
        source_commit: PI_SOURCE_COMMIT.into(),
        license: PI_LICENSE.into(),
        node_version: PI_NODE_MINIMUM.into(),
        entrypoint,
        runtime: node,
        fixed_args: PI_RPC_ARGS.map(str::to_owned).to_vec(),
    };
    let binding = curated
        .validate_candidate(&candidate)
        .expect("research binding");
    assert!(matches!(
        PiRuntime::from_binding(
            &binding,
            [1; 32],
            RuntimeEpoch::new(1).expect("epoch"),
            SageAgentSessionId::parse("session-pi-launch").expect("session")
        ),
        Err(AdapterRuntimeError::LaunchUnavailable)
    ));
}

#[test]
fn initialize_is_strictly_staged_and_delivery_outcomes_do_not_replay() {
    let mut ordered = fixture_runtime(1);
    let first = ordered.command(AdapterCommand::Initialize).expect("first");
    let token = first.into_token();
    assert_eq!(
        ordered.resolve_egress(token.clone(), EgressResolution::Written),
        Err(AdapterRuntimeError::EgressIntent)
    );
    ordered
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    ordered
        .resolve_egress(token, EgressResolution::Written)
        .expect("written after queue");

    let mut rejected = fixture_runtime(2);
    let first = rejected.command(AdapterCommand::Initialize).expect("first");
    rejected
        .resolve_egress(first.into_token(), EgressResolution::QueueRejected)
        .expect("queue rollback");
    assert!(rejected.command(AdapterCommand::Initialize).is_ok());

    let mut zero = fixture_runtime(3);
    let first = zero.command(AdapterCommand::Initialize).expect("first");
    let token = first.into_token();
    zero.resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    zero.resolve_egress(token, EgressResolution::WriteZeroBytes)
        .expect("zero-byte rollback");
    assert!(zero.command(AdapterCommand::Initialize).is_ok());

    let mut unknown = fixture_runtime(4);
    let first = unknown.command(AdapterCommand::Initialize).expect("first");
    unknown
        .resolve_egress(first.into_token(), EgressResolution::DeliveryUnknown)
        .expect("unknown recorded");
    assert!(matches!(
        unknown.command(AdapterCommand::Initialize),
        Err(AdapterRuntimeError::Operation)
    ));

    let mut malformed = fixture_runtime(11);
    malformed
        .command(AdapterCommand::Initialize)
        .expect("first");
    assert!(matches!(
        malformed.ingest(&frame(json!("invalid")), &Mapper),
        Err(AdapterRuntimeError::Frame)
    ));
    assert!(matches!(
        malformed.command(AdapterCommand::Initialize),
        Err(AdapterRuntimeError::Operation)
    ));

    let mut forged = fixture_runtime(5);
    forged.command(AdapterCommand::Initialize).expect("first");
    assert!(matches!(
        forged.ingest(
            &frame(json!({
                "type":"response","id":"pi-5-2","command":"get_available_models",
                "success":true,"data":{"models":[]}
            })),
            &Mapper
        ),
        Err(AdapterRuntimeError::Frame)
    ));
}

#[test]
fn lifecycle_models_text_tools_usage_cancel_and_unavailable_commands_are_normalized() {
    let mut runtime = fixture_runtime(6);
    initialize(&mut runtime);
    let capabilities = runtime.capabilities().expect("capabilities");
    assert!(capabilities.new_session && capabilities.prompt && capabilities.cancel);
    assert!(
        !capabilities.resume_session && !capabilities.load_session && !capabilities.close_session
    );
    for command in [
        AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("id", "pi-session").expect("id"),
            native_root: r"C:\repo".into(),
            model: None,
        },
        AdapterCommand::LoadSession {
            external_session_id: ExternalId::new("id", "pi-session").expect("id"),
        },
        AdapterCommand::CloseSession,
    ] {
        assert!(matches!(
            runtime.command(command),
            Err(AdapterRuntimeError::UnsupportedCommand)
        ));
    }
    assert!(matches!(
        runtime.command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("model", "other:model").expect("model")),
        }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    let session = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\private\must-not-enter-wire".into(),
            model: Some(RequiredText::new("model", "openai:gpt").expect("model")),
        })
        .expect("new session");
    assert!(!String::from_utf8_lossy(session.frame().payload()).contains("private"));
    let value = deliver(&mut runtime, session);
    runtime
        .ingest(&response(&value, Value::Null), &Mapper)
        .expect("session");
    running(&mut runtime);

    let start = runtime
        .ingest(&frame(json!({"type":"message_start"})), &Mapper)
        .expect("text start");
    let AdapterEvent::Item { id, .. } = &start.events()[0] else {
        panic!("item")
    };
    assert_eq!(
        id.turn_client_message_id.as_ref().expect("client").as_str(),
        "client-pi-1"
    );
    assert!(matches!(
        runtime
            .ingest(
                &frame(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"hello"}})),
                &Mapper
            )
            .expect("delta")
            .events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::Text { .. },
            ..
        }]
    ));
    let second = runtime
        .ingest(
            &frame(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":" world"}})),
            &Mapper,
        )
        .expect("second delta");
    assert!(matches!(
        second.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::Text { text },
            ..
        }] if text.as_str() == "hello world"
    ));
    let completed = runtime
        .ingest(&frame(json!({"type":"message_end"})), &Mapper)
        .expect("text end");
    assert!(matches!(
        completed.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::Text { text },
            ..
        }] if text.as_str() == "hello world"
    ));
    assert!(matches!(
        runtime
            .ingest(
                &frame(json!({
                    "type":"tool_execution_start","toolCallId":"read-1","toolName":"read"
                })),
                &Mapper
            )
            .expect("tool")
            .events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { .. },
            ..
        }]
    ));
    runtime
        .ingest(
            &frame(json!({
                "type":"tool_execution_end","toolCallId":"read-1","isError":false
            })),
            &Mapper,
        )
        .expect("tool end");
    let cancel = runtime.command(AdapterCommand::Cancel).expect("cancel");
    assert!(cancel.token().is_control() && cancel.token().is_cancel());
    assert_eq!(request(&cancel)["type"], "abort");
    deliver(&mut runtime, cancel);
    let ended = runtime
        .ingest(
            &frame(json!({"type":"agent_end","messages":[{"role":"assistant","content":[],"stopReason":"aborted","usage":{"input":2,"output":1}}],"willRetry":false})),
            &Mapper,
        )
        .expect("agent end");
    assert!(matches!(ended.events(), [AdapterEvent::Usage { .. }]));
    let terminal = runtime
        .ingest(&frame(json!({"type":"agent_settled"})), &Mapper)
        .expect("settled");
    assert!(matches!(terminal.events(), [AdapterEvent::Turn { .. }]));
}

#[test]
fn allow_once_on_pi_does_not_emit_denial_and_records_resolved_allowed() {
    let mut runtime = fixture_runtime(7);
    ready(&mut runtime);
    running(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"extension_ui_request","id":"write-1","method":"select",
                "title":"Allow tool?","options":["Allow","Block"]
            })),
            &Mapper,
        )
        .expect("permission");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::PermissionRequest {
            payload: NormalizedPayload::PermissionRequest {
                category: crate::acp::intent::ToolCategory::Other,
                ..
            },
            ..
        }]
    ));
    assert!(batch.egress().is_empty());
    let AdapterEvent::PermissionRequest {
        external_request_id,
        ..
    } = &batch.events()[0]
    else {
        panic!("permission request")
    };
    assert_eq!(external_request_id.as_str(), "write-1");
    let allow = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("write-1").expect("request"),
            decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
        })
        .expect("allow once");
    assert!(!allow.token().is_mandatory());
    assert_eq!(allow.token().allow_request(), Some("write-1"));
    let body = request(&allow);
    assert_eq!(body["type"], "extension_ui_response");
    assert_eq!(body["id"], "write-1");
    assert_eq!(body["value"], "Allow");
    runtime
        .resolve_egress(allow.token().clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(allow.into_token(), EgressResolution::Written)
        .expect("written");
}

#[test]
fn allow_once_on_pi_rejects_an_ambiguous_select_option() {
    let mut runtime = fixture_runtime(13);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(
            &frame(json!({
                "type":"extension_ui_request","id":"disallow-1","method":"select",
                "options":["Disallow","Block"]
            })),
            &Mapper,
        )
        .expect("permission");
    assert!(matches!(
        runtime.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("disallow-1").expect("request"),
            decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
        }),
        Err(AdapterRuntimeError::LaterPermissionMiss)
    ));
}

#[test]
fn durable_permission_id_correlates_to_pi_request_id() {
    let mut runtime = fixture_runtime(12);
    ready(&mut runtime);
    running(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"extension_ui_request","id":"pi-correlation","method":"confirm",
                "title":"Allow tool?","message":"Allow write?"
            })),
            &Mapper,
        )
        .expect("permission");
    let AdapterEvent::PermissionRequest {
        external_request_id,
        ..
    } = &batch.events()[0]
    else {
        panic!("permission request")
    };
    assert_eq!(external_request_id.as_str(), "pi-correlation");
}

#[test]
fn later_decision_uses_the_shared_helper_not_permission_intent_reject() {
    let mut runtime = fixture_runtime(11);
    assert!(matches!(
        runtime.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("request-pi-missing").expect("request"),
            decision: crate::acp::runtime::permissions::PermissionDecision::Deny,
        }),
        Err(AdapterRuntimeError::LaterPermissionMiss)
    ));
}

#[test]
fn malformed_unknown_and_capacity_plus_one_are_bounded() {
    let mut runtime = fixture_runtime(9);
    assert!(matches!(
        runtime.ingest(&frame(json!("invalid")), &Mapper),
        Err(AdapterRuntimeError::Frame)
    ));
    ready(&mut runtime);
    running(&mut runtime);
    assert!(matches!(
        runtime
            .ingest(&frame(json!({"type":"future_event"})), &Mapper)
            .expect("unknown")
            .events(),
        [AdapterEvent::Unsupported(_)]
    ));

    let mut full = fixture_runtime(10);
    initialize(&mut full);
    for _ in 0..16 {
        full.command(AdapterCommand::ModelPage { cursor: None })
            .expect("bounded request");
    }
    assert!(matches!(
        full.command(AdapterCommand::ModelPage { cursor: None }),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn text_snapshot_plus_one_poison_and_prompt_rollback_clear_state() {
    let mut rollback = fixture_runtime(12);
    ready(&mut rollback);
    let prompt = rollback
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "rollback").expect("prompt"),
            client_message_id: RequiredText::new("client", "rollback-client").expect("client"),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .expect("prompt");
    rollback
        .resolve_egress(prompt.into_token(), EgressResolution::QueueRejected)
        .expect("rollback");
    assert!(rollback.active_turn.is_none());

    let mut bounded = fixture_runtime(13);
    ready(&mut bounded);
    running(&mut bounded);
    bounded
        .ingest(&frame(json!({"type":"message_start"})), &Mapper)
        .expect("start");
    bounded
        .ingest(
            &frame(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"x".repeat(262_144)}})),
            &Mapper,
        )
        .expect("exact bound");
    assert!(matches!(
        bounded.ingest(
            &frame(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"x"}})),
            &Mapper,
        ),
        Err(AdapterRuntimeError::BatchBytes)
    ));
    assert!(
        bounded
            .active_turn
            .as_ref()
            .is_some_and(|turn| turn.text_snapshot.is_none())
    );
    assert!(matches!(
        bounded.command(AdapterCommand::Cancel),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn split_chunk_secrets_and_paths_are_redacted_after_snapshot_join() {
    let mut runtime = fixture_runtime(14);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(&frame(json!({"type":"message_start"})), &Mapper)
        .expect("start");
    let mut rendered = String::new();
    for delta in [
        "Bearer ",
        "token-secret ",
        "C:",
        r"\Users\private ",
        "sk-",
        "private",
    ] {
        let batch = runtime
            .ingest(
                &frame(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":delta}})),
                &Mapper,
            )
            .expect("chunk");
        rendered = format!("{:?}", batch.events());
    }
    assert!(rendered.contains("Bearer"));
    assert!(!rendered.contains("token-secret"));
    assert!(!rendered.contains("Users"));
    assert!(!rendered.contains("private"));
    assert!(!rendered.contains("sk-"));
}
