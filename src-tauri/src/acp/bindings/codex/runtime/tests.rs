use sage_core::error::AppError;

use crate::acp::bindings::codex::test_identity::{MANAGED_CODEX_HOME, user_agent, version_line};
use crate::acp_protocol::codex_protocol::{OBSERVED_PROTOCOL_IDENTITY, PINNED_PROTOCOL_IDENTITY};
use serde_json::{Value, json};

use super::*;
use crate::project_registry::ProjectRootAuthority;
use crate::{
    acp::{
        bindings::{
            adapter::{
                AdapterCommand, AdapterEvent, AdapterIdentity, AdapterRuntimeError,
                EgressResolution, HarnessAdapter, ResourceMapper,
            },
            codex::{
                CodexProjectRootAuthority, Platform, parse_packet_metadata, parse_security_policy,
            },
        },
        contract::{
            ProjectGeneration, RequiredText, RuntimeEpoch, SafeText, SageAgentSessionId,
            SageProjectId, SageRootId,
        },
        intent::{ExternalId, NormalizedPayload, Prompt, SafeResourceRef},
        lifecycle::ItemLifecycle,
        transport::{Frame, IncrementalFrameDecoder},
    },
    acp_protocol::permissions::PermissionDecision,
};
use crate::{persistence::Database, state::AppState};

mod background_terminal_tests;
mod browser_mcp_tests;
mod duplicate_terminal_probe;
mod foreign_thread_tests;
mod fork_tests;
mod hook_and_progress_events;
mod mcp_tests;
mod model_choice;
mod model_identity;
#[cfg(feature = "acp-fixture")]
mod notice_journey_tests;
mod notice_tests;
mod remaining_events;
mod revert_tests;
mod saved_diff_tests;
mod session_title_tests;
mod skill_command_tests;
#[cfg(feature = "acp-fixture")]
mod skills_journey_tests;
#[cfg(feature = "acp-fixture")]
mod steer_journey_tests;
mod usage_tests;

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        (absolute_path == r"C:\repo\src\lib.rs")
            .then(|| {
                SafeResourceRef::new(
                    SageRootId::parse("root-runtime").expect("root"),
                    "src/lib.rs",
                )
                .expect("resource")
            })
            .ok_or(crate::acp::intent::UnavailableResourceReason::Foreign)
    }
}

fn fixture_runtime() -> CodexRuntime {
    CodexRuntime::fixture(Platform::WindowsX86_64)
}

#[cfg(windows)]
fn observed_cli_path(label: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("{label}-{}", uuid::Uuid::new_v4()));
    let package = directory.join("node_modules/@openai/codex");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("package.json"), br#"{"name":"@openai/codex"}"#).unwrap();
    let path = directory.join("codex.cmd");
    std::fs::write(&path, "@echo off\r\n").unwrap();
    path
}

fn launch_project_id() -> SageProjectId {
    SageProjectId::parse("project-codex-launch").unwrap()
}

fn launch_root_id() -> SageRootId {
    SageRootId::parse("root-codex-launch").unwrap()
}

fn launch_root(
    project_id: SageProjectId,
    generation: ProjectGeneration,
    root_id: SageRootId,
) -> CodexProjectRootAuthority {
    CodexProjectRootAuthority::for_test(
        ProjectRootAuthority::for_test(
            project_id,
            generation,
            root_id,
            &std::env::current_dir().unwrap(),
        )
        .unwrap(),
    )
}

#[test]
fn codex_authority_accepts_exactly_one_registered_root_without_process_admission() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let base =
            std::env::temp_dir().join(format!("sage-codex-one-root-{}", uuid::Uuid::new_v4()));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let state = AppState::new(database.clone());
        let project_id = SageProjectId::parse(project.id).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();
        let before = crate::acp::process::TreeCapacityPool::global().usage();

        let authority = database.codex_project_root_authority(&context).unwrap();

        assert_eq!(authority.project_id(), &project_id);
        assert_eq!(authority.root_id().as_str(), project.roots[0].id);
        assert_eq!(
            crate::acp::process::TreeCapacityPool::global().usage(),
            before
        );
        drop(authority);
        drop(context);
        let _ = std::fs::remove_dir_all(base);
    });
}

#[test]
fn codex_authority_rejects_two_registered_roots_without_process_admission() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let base =
            std::env::temp_dir().join(format!("sage-codex-two-roots-{}", uuid::Uuid::new_v4()));
        let first = base.join("first");
        let second = base.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&first).unwrap();
        database.add_project_root(&project.id, &second).unwrap();
        let state = AppState::new(database.clone());
        let project_id = SageProjectId::parse(project.id).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();
        let before = crate::acp::process::TreeCapacityPool::global().usage();

        assert!(matches!(
            database.codex_project_root_authority(&context),
            Err(AppError::HarnessUnavailable(_))
        ));
        assert_eq!(
            crate::acp::process::TreeCapacityPool::global().usage(),
            before
        );
        drop(context);
        let _ = std::fs::remove_dir_all(base);
    });
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

fn request_id(egress: &AdapterEgress) -> u64 {
    serde_json::from_slice::<Value>(egress.frame().payload()).expect("outbound json")["id"]
        .as_u64()
        .expect("request id")
}

fn initialize(runtime: &mut CodexRuntime) {
    let egress = runtime
        .command(AdapterCommand::Initialize)
        .expect("initialize");
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .expect("queued");
    let batch = runtime
        .ingest(
            &frame(json!({"id":id,"result":{
                "userAgent":user_agent("sage", "unknown"),
                "codexHome":MANAGED_CODEX_HOME,
                "platformFamily":"windows",
                "platformOs":"windows"
            }})),
            &Mapper,
        )
        .expect("initialize response");
    assert!(matches!(batch.events(), [AdapterEvent::Initialized { .. }]));
    assert_eq!(batch.egress().len(), 1);
    assert!(!batch.egress()[0].token().is_mandatory());
    runtime
        .resolve_egress(batch.egress()[0].token().clone(), EgressResolution::Queued)
        .expect("initialized notification queued");
}

fn ready(runtime: &mut CodexRuntime) {
    ready_with_reported_config(runtime, None, None);
}

fn ready_with_reported_config(
    runtime: &mut CodexRuntime,
    model: Option<&str>,
    effort: Option<&str>,
) {
    initialize(runtime);
    let egress = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .expect("new session");
    let id = request_id(&egress);
    runtime
        .ingest(
            &frame(json!({"id":id,"result":{"thread":{"id":"thread-1"},"model":model,"reasoningEffort":effort}})),
            &Mapper,
        )
        .expect("session response");
}

#[test]
fn bound_turn_applies_catalog_pages_even_before_prompt_dispatch() {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("selected"), Some("high"));
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime.finish_model_config_hydration();
    let batch = runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{
                "data":[{"id":"selected","displayName":"Selected","isDefault":true,
                    "defaultReasoningEffort":"high","supportedReasoningEfforts":[
                        {"reasoningEffort":"high","description":"Deep"}]}],"nextCursor":"page-2"
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert!(batch.egress().is_empty());
    assert_eq!(
        serde_json::to_value(runtime.session_config().unwrap()).unwrap()["configOptions"][0]["currentValue"],
        "high"
    );
    assert_eq!(
        runtime.selected_model.as_ref().unwrap().id.as_str(),
        "selected"
    );
    assert!(
        runtime
            .command(AdapterCommand::ModelPage { cursor: None })
            .is_err()
    );
}

#[test]
fn catalog_rpc_failure_is_ignored_and_session_stays_promptable() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    let batch = runtime
        .ingest(
            &frame(json!({"id":list_id,"error":{"code":-32601,"message":"Method not found"}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert!(batch.egress().is_empty());
    assert!(runtime.session_config().is_none());
    assert!(
        runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "continue").unwrap(),
                client_message_id: RequiredText::new("client", "catalog-failure").unwrap(),
                access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .is_ok()
    );
}

#[test]
fn codex_session_requests_model_efforts_after_becoming_ready() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("modelId", "gpt-6-astra").unwrap()),
        })
        .unwrap();
    let batch = runtime
        .ingest(
            &frame(json!({"id":request_id(&start),"result":{"thread":{"id":"thread-1"}}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        batch.events(),
        [AdapterEvent::SessionReady { .. }]
    ));
    assert!(batch.egress().is_empty());
    let hydration = runtime.model_config_hydration().unwrap().unwrap();
    let request: Value = serde_json::from_slice(hydration.frame().payload()).unwrap();
    assert_eq!(request["method"], "model/list");
}

#[test]
fn codex_model_list_exposes_supported_thinking_levels() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("modelId", "gpt-6-astra").unwrap()),
        })
        .unwrap();
    let ready = runtime
        .ingest(
            &frame(json!({"id":request_id(&start),"result":{"thread":{"id":"thread-1"},"model":"gpt-6-astra","reasoningEffort":"medium"}})),
            &Mapper,
        )
        .unwrap();
    assert!(ready.egress().is_empty());
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[{
        "id":"gpt-6-astra","displayName":"GPT-6 Astra","isDefault":true,
        "defaultReasoningEffort":"medium",
        "supportedReasoningEfforts":[
            {"reasoningEffort":"low","description":"Quick"},
            {"reasoningEffort":"medium","description":"Balanced"},
            {"reasoningEffort":"high","description":"Deep"}
        ]
    }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(config["configOptions"][0]["category"], "effort");
    assert_eq!(config["configOptions"][0]["currentValue"], "medium");
    assert_eq!(config["configOptions"][0]["options"][2]["value"], "high");
}

#[test]
fn codex_model_without_efforts_hides_thinking_config() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("modelId", "gpt-6-astra").unwrap()),
        })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request_id(&start),"result":{"thread":{"id":"thread-1"},"model":"gpt-6-astra"}})),
            &Mapper,
        )
        .unwrap();
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[{
        "id":"gpt-6-astra","displayName":"GPT-6 Astra","isDefault":true,
        "defaultReasoningEffort":"medium","supportedReasoningEfforts":[]
    }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    let config = runtime.session_config().unwrap();
    let wire = serde_json::to_value(&config).unwrap();
    assert_eq!(wire["configOptions"].as_array().unwrap().len(), 1);
    assert_eq!(wire["configOptions"][0]["category"], "model");
    assert_eq!(wire["configOptions"][0]["currentValue"], "gpt-6-astra");
    assert_eq!(
        config.confirmed_model.unwrap().name.unwrap().as_str(),
        "GPT-6 Astra"
    );
}

#[test]
fn model_efforts_accept_the_exact_collection_limit() {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("model"), Some("effort-0"));
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    let efforts: Vec<_> = (0..32)
        .map(|index| {
            json!({
                "reasoningEffort":format!("effort-{index}"),"description":"Deep"
            })
        })
        .collect();
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[{
        "id":"model","displayName":"Model","isDefault":true,
        "defaultReasoningEffort":"effort-0","supportedReasoningEfforts":efforts
    }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(
        config["configOptions"][0]["options"]
            .as_array()
            .unwrap()
            .len(),
        32
    );
}

#[test]
fn model_effort_pages_with_invalid_fields_or_collections_are_ignored_and_stop_paging() {
    for efforts in [
        json!([{"reasoningEffort":"e".repeat(513),"description":"Deep"}]),
        json!([{"reasoningEffort":"high","description":"Deep\0"}]),
        json!((0..33).map(|index| json!({"reasoningEffort":format!("effort-{index}"),"description":"Deep"})).collect::<Vec<_>>()),
        json!({"reasoningEffort":"high","description":"not an array"}),
    ] {
        for partial in [false, true] {
            let (mut runtime, list_id) = model_page_rejection::runtime_waiting_for_effort_page(partial);
            model_page_rejection::assert_invalid_model_page_is_ignored(
                &mut runtime, list_id, json!({"data":[{
                    "id":"model","displayName":"Model","isDefault":true,
                    "supportedReasoningEfforts":efforts
                }],"nextCursor":"third"}),
            );
        }
    }
}

#[test]
fn over_budget_model_effort_pages_are_ignored_and_stop_paging() {
    let efforts: Vec<Value> = (0..17)
        .map(|index| {
            json!({
                "reasoningEffort":format!("effort-{index}"),
                "description":"x".repeat(8_192)
            })
        })
        .collect();
    for partial in [false, true] {
        let (mut runtime, list_id) = model_page_rejection::runtime_waiting_for_effort_page(partial);
        model_page_rejection::assert_invalid_model_page_is_ignored(
            &mut runtime,
            list_id,
            json!({"data":[{
                "id":"gpt-6-astra","displayName":"GPT-6 Astra","isDefault":true,
                "defaultReasoningEffort":"effort-0",
                "supportedReasoningEfforts":efforts
            }],"nextCursor":"third"}),
        );
    }
}

#[test]
fn resumed_thread_without_requested_model_uses_actual_model_efforts() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let resume = runtime
        .command(AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("thread", "thread-1").unwrap(),
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request_id(&resume),"result":{
                "thread":{"id":"thread-1"},"model":"non-default","reasoningEffort":"high"
            }})),
            &Mapper,
        )
        .unwrap();
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[{
                "id":"default","displayName":"Default","isDefault":true,
                "defaultReasoningEffort":"medium",
                "supportedReasoningEfforts":[{"reasoningEffort":"medium","description":"Balanced"}]
            },{
                "id":"non-default","displayName":"Non-default","isDefault":false,
                "defaultReasoningEffort":"low",
                "supportedReasoningEfforts":[
                    {"reasoningEffort":"low","description":"Fast"},
                    {"reasoningEffort":"high","description":"Deep"}
                ]
            }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(config["configOptions"][0]["currentValue"], "high");
    assert_eq!(config["configOptions"][0]["options"][0]["value"], "low");
    assert_eq!(config["configOptions"][0]["options"][1]["value"], "high");
}

#[test]
fn resumed_codex_thread_uses_reported_effort_not_catalog_default() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let resume = runtime
        .command(AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("thread", "thread-1").unwrap(),
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("modelId", "gpt-6-astra").unwrap()),
        })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request_id(&resume),"result":{
                "thread":{"id":"thread-1"},"model":"gpt-6-astra","reasoningEffort":"high"
            }})),
            &Mapper,
        )
        .unwrap();
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[{
        "id":"gpt-6-astra","displayName":"GPT-6 Astra","isDefault":true,
        "defaultReasoningEffort":"medium",
        "supportedReasoningEfforts":[
            {"reasoningEffort":"medium","description":"Balanced"},
            {"reasoningEffort":"high","description":"Deep"}
        ]
    }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(runtime.session_config().unwrap()).unwrap()["configOptions"][0]["currentValue"],
        "high"
    );
}

#[test]
fn resumed_codex_thread_without_reported_effort_hides_thinking_config() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let resume = runtime
        .command(AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("thread", "thread-1").unwrap(),
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("modelId", "gpt-6-astra").unwrap()),
        })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request_id(&resume),"result":{
                "thread":{"id":"thread-1"},"model":"gpt-6-astra"
            }})),
            &Mapper,
        )
        .unwrap();
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[{
        "id":"gpt-6-astra","displayName":"GPT-6 Astra","isDefault":true,
        "defaultReasoningEffort":"medium",
        "supportedReasoningEfforts":[
            {"reasoningEffort":"medium","description":"Balanced"},
            {"reasoningEffort":"high","description":"Deep"}
        ]
    }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    let config = runtime.session_config().unwrap();
    let wire = serde_json::to_value(&config).unwrap();
    assert_eq!(wire["configOptions"].as_array().unwrap().len(), 1);
    assert_eq!(wire["configOptions"][0]["category"], "model");
    assert_eq!(wire["configOptions"][0]["currentValue"], "gpt-6-astra");
    assert_eq!(
        config.confirmed_model.unwrap().name.unwrap().as_str(),
        "GPT-6 Astra"
    );
}

#[test]
fn starting_resume_clears_model_page_effort_until_current_effort_is_known() {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("gpt-6-astra"), Some("medium"));
    let list = runtime
        .command(AdapterCommand::ModelPage { cursor: None })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request_id(&list),"result":{"data":[{
        "id":"gpt-6-astra","displayName":"GPT-6 Astra","isDefault":true,
        "defaultReasoningEffort":"medium",
        "supportedReasoningEfforts":[{"reasoningEffort":"medium","description":"Balanced"}]
    }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        runtime
            .session_config()
            .unwrap()
            .config_options
            .as_slice()
            .len(),
        2
    );
    let resume = runtime
        .command(AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("thread", "thread-1").unwrap(),
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("modelId", "gpt-6-astra").unwrap()),
        })
        .unwrap();
    assert_eq!(
        runtime
            .session_config()
            .map_or(0, |config| config.config_options.as_slice().len()),
        0
    );
    runtime
        .ingest(
            &frame(json!({"id":request_id(&resume),"result":{
                "thread":{"id":"thread-1"}
            }})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        runtime
            .session_config()
            .map_or(0, |config| config.config_options.as_slice().len()),
        0
    );
}

#[test]
fn session_effort_survives_thread_started_before_or_after_response() {
    for (resume_session, started_first) in
        [(false, true), (false, false), (true, true), (true, false)]
    {
        let mut runtime = fixture_runtime();
        initialize(&mut runtime);
        let command = if resume_session {
            AdapterCommand::ResumeSession {
                external_session_id: ExternalId::new("thread", "thread-1").unwrap(),
                native_root: r"C:\repo".into(),
                model: Some(RequiredText::new("modelId", "gpt-6-astra").unwrap()),
            }
        } else {
            AdapterCommand::NewSession {
                native_root: r"C:\repo".into(),
                model: Some(RequiredText::new("modelId", "gpt-6-astra").unwrap()),
            }
        };
        let request = runtime.command(command).unwrap();
        let started = frame(json!({"method":"thread/started","params":{
            "thread":{"id":"thread-1"}
        }}));
        let response = frame(json!({"id":request_id(&request),"result":{
            "thread":{"id":"thread-1"},"model":"gpt-6-astra","reasoningEffort":"high"
        }}));
        if started_first {
            let notification = runtime.ingest(&started, &Mapper).unwrap();
            assert!(
                notification
                    .events()
                    .iter()
                    .all(|event| !matches!(event, AdapterEvent::SessionReady { .. }))
            );
            let ready = runtime.ingest(&response, &Mapper).unwrap();
            assert!(matches!(
                ready.events(),
                [AdapterEvent::SessionReady { .. }]
            ));
        } else {
            let ready = runtime.ingest(&response, &Mapper).unwrap();
            assert!(matches!(
                ready.events(),
                [AdapterEvent::SessionReady { .. }]
            ));
            let notification = runtime.ingest(&started, &Mapper).unwrap();
            assert!(matches!(notification.events(), [AdapterEvent::IgnoredLate]));
        }
        let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
        runtime
            .ingest(
                &frame(json!({"id":list_id,"result":{"data":[{
            "id":"gpt-6-astra","displayName":"GPT-6 Astra","isDefault":true,
            "defaultReasoningEffort":"medium",
            "supportedReasoningEfforts":[
                {"reasoningEffort":"medium","description":"Balanced"},
                {"reasoningEffort":"high","description":"Deep"}
            ]
        }],"nextCursor":null}})),
                &Mapper,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(runtime.session_config().unwrap()).unwrap()["configOptions"][0]["currentValue"],
            "high"
        );
    }
}

#[test]
fn catalog_effort_byte_bound_survives_configuration_and_turn_start() {
    use crate::acp::{
        contract::SageChatId,
        scheduler::RuntimeOwner,
        session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigRevision, ConfigValue},
    };
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("model"), Some("medium"));
    let effort = "e".repeat(512);
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[{
        "id":"model","displayName":"Model","isDefault":true,
        "defaultReasoningEffort":"medium","supportedReasoningEfforts":[
            {"reasoningEffort":"medium","description":"Balanced"},
            {"reasoningEffort":effort,"description":"Deep"}
        ]
    }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    let intent = ConfigIntent {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-1").unwrap(),
            chat_id: SageChatId::parse("chat-1").unwrap(),
            session_id: SageAgentSessionId::parse("session-codex-runtime").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        },
        expected_config_revision: ConfigRevision::first(),
        mutation: ConfigMutation::Option {
            config_id: ConfigId::new("configId", "effort").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", effort.clone()).unwrap(),
            },
        },
    };
    assert!(runtime.configure(&intent).unwrap().is_none());
    let before = runtime.session_config().unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap()["configOptions"][0]["currentValue"],
        effort
    );
    let oversized = ConfigIntent {
        mutation: ConfigMutation::Option {
            config_id: ConfigId::new("configId", "effort").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", format!("{effort}x")).unwrap(),
            },
        },
        ..intent
    };
    assert!(runtime.configure(&oversized).is_err());
    assert_eq!(runtime.session_config().unwrap(), before);
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "keep full bounded effort").unwrap(),
            client_message_id: RequiredText::new("client", "bounded-effort").unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let request: Value = serde_json::from_slice(prompt.frame().payload()).unwrap();
    assert_eq!(request["params"]["effort"], effort);
}

#[test]
fn codex_thinking_change_is_local_and_reaches_the_next_turn_start() {
    use crate::acp::{
        contract::SageChatId,
        scheduler::RuntimeOwner,
        session_config::{ConfigIntent, ConfigMutation, ConfigRevision, ConfigValue},
    };
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("modelId", "gpt-6-astra").unwrap()),
        })
        .unwrap();
    let ready = runtime
        .ingest(
            &frame(json!({"id":request_id(&start),"result":{"thread":{"id":"thread-1"},"model":"gpt-6-astra","reasoningEffort":"medium"}})),
            &Mapper,
        )
        .unwrap();
    assert!(ready.egress().is_empty());
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[{
        "id":"gpt-6-astra","displayName":"GPT-6 Astra","isDefault":true,
        "defaultReasoningEffort":"medium",
        "supportedReasoningEfforts":[
            {"reasoningEffort":"medium","description":"Balanced"},
            {"reasoningEffort":"high","description":"Deep"}
        ]
    }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    let intent = ConfigIntent {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-1").unwrap(),
            chat_id: SageChatId::parse("chat-1").unwrap(),
            session_id: SageAgentSessionId::parse("session-codex-runtime").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        },
        expected_config_revision: ConfigRevision::first(),
        mutation: ConfigMutation::Option {
            config_id: RequiredText::new("configId", "effort").unwrap(),
            value: ConfigValue::Select {
                value: RequiredText::new("value", "high").unwrap(),
            },
        },
    };
    let channel = std::sync::Arc::new(crate::acp::session_config::ConfigChannel::default());
    channel.reserve(intent.owner.clone());
    channel
        .update(&intent.owner, runtime.session_config().unwrap())
        .unwrap();
    channel.publish(&intent.owner);
    let owner = intent.owner.clone();
    let mut runtime = crate::acp::bindings::runtime::NativeRuntime::new(runtime);
    runtime.attach_config(owner.clone(), std::sync::Arc::clone(&channel));
    assert!(runtime.configure(&intent).unwrap().is_none());
    assert_eq!(
        serde_json::to_value(channel.snapshot(&owner).unwrap().config).unwrap()["configOptions"][0]
            ["currentValue"],
        "high"
    );
    assert!(!runtime.config_pending());
    let invalid = ConfigIntent {
        expected_config_revision: channel.snapshot(&owner).unwrap().config_revision,
        mutation: ConfigMutation::Option {
            config_id: RequiredText::new("configId", "effort").unwrap(),
            value: ConfigValue::Select {
                value: RequiredText::new("value", "unsupported").unwrap(),
            },
        },
        ..intent
    };
    assert!(runtime.configure(&invalid).is_err());
    let page = runtime
        .command(AdapterCommand::ModelPage { cursor: None })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request_id(&page),"result":{"data":[{
        "id":"gpt-6-astra","displayName":"GPT-6 Astra","isDefault":true,
        "defaultReasoningEffort":"medium",
        "supportedReasoningEfforts":[
            {"reasoningEffort":"medium","description":"Balanced"},
            {"reasoningEffort":"high","description":"Deep"}
        ]
    }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(runtime.session_config().unwrap()).unwrap()["configOptions"][0]["currentValue"],
        "high"
    );
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "use selected effort").unwrap(),
            client_message_id: RequiredText::new("client", "effort-turn").unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let request: Value = serde_json::from_slice(prompt.frame().payload()).unwrap();
    assert_eq!(request["method"], "turn/start");
    assert_eq!(request["params"]["effort"], "high");
    let schema: Value = serde_json::from_slice(include_bytes!(
        "../../../../../testdata/acp/codex/observed-effort-schema.json"
    ))
    .unwrap();
    let methods = schema["clientRequestMethods"].as_array().unwrap();
    assert!(methods.contains(&request["method"]));
    assert!(!methods.contains(&json!("thread/settings/update")));
    assert_eq!(schema["reasoningEffort"]["type"], "string");
}

#[test]
fn thread_started_notification_waits_for_the_pending_new_session_response() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let request = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .expect("new session");

    let batch = runtime
        .ingest(
            &frame(json!({"method":"thread/started","params":{
                "thread":{"id":"thread-native"}
            }})),
            &Mapper,
        )
        .expect("pinned Codex thread-started notification");

    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert!(
        batch.egress().is_empty(),
        "thread notification does not send a Goal RPC before the session response"
    );
    let ready = runtime
        .ingest(
            &frame(json!({"id":request_id(&request),"result":{"thread":{"id":"thread-native"}}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        ready.events(),
        [AdapterEvent::SessionReady { .. }]
    ));
}

#[test]
fn thread_started_notification_after_start_response_is_ignored_late() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let request = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .expect("new session");
    let id = request_id(&request);
    runtime
        .ingest(
            &frame(json!({"id":id,"result":{"thread":{"id":"thread-native"}}})),
            &Mapper,
        )
        .expect("thread-start response");

    let late = runtime
        .ingest(
            &frame(json!({"method":"thread/started","params":{
                "thread":{"id":"thread-native"}
            }})),
            &Mapper,
        )
        .expect("late pinned Codex thread-started notification");

    assert!(matches!(late.events(), [AdapterEvent::IgnoredLate]));
}

fn running_turn(runtime: &mut CodexRuntime) {
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-1").expect("client"),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .expect("prompt");
    let id = request_id(&egress);
    runtime
        .ingest(
            &frame(json!({"id":id,"result":{"turn":{"id":"turn-1"}}})),
            &Mapper,
        )
        .expect("prompt response");
    runtime
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"thread-1","turn":{"id":"turn-1"}
            }})),
            &Mapper,
        )
        .expect("turn start");
}

#[test]
#[cfg(windows)]
fn steer_targets_only_the_active_codex_turn() {
    use std::{fs, sync::Arc};

    let path = observed_cli_path("sage-observed-codex-steer");
    let binding = crate::acp::bindings::codex::AdmittedCodexBinding::admit_observed_for_test(
        path.clone(),
        version_line("codex-cli"),
    )
    .unwrap();
    let mut runtime = CodexRuntime::from_admitted_binding(
        Arc::new(binding),
        launch_root(
            launch_project_id(),
            ProjectGeneration::ZERO,
            launch_root_id(),
        ),
        RuntimeEpoch::new(24).unwrap(),
        SageAgentSessionId::parse("session-observed-steer").unwrap(),
    )
    .unwrap();
    let init = runtime.command(AdapterCommand::Initialize).unwrap();
    let init_id = request_id(&init);
    runtime
        .resolve_egress(init.into_token(), EgressResolution::Queued)
        .unwrap();
    let initialized = runtime
        .ingest(
            &frame(json!({"id":init_id,"result":{
                "userAgent":user_agent("Codex Desktop", "dumb"),
                "codexHome":r"C:\Users\test\.codex",
                "platformFamily":"windows","platformOs":"windows"
            }})),
            &Mapper,
        )
        .unwrap();
    let [AdapterEvent::Initialized { capabilities }] = initialized.events() else {
        panic!("observed initialize publishes capabilities");
    };
    assert!(capabilities.active_turn_steer);
    runtime
        .resolve_egress(
            initialized.egress()[0].token().clone(),
            EgressResolution::Queued,
        )
        .unwrap();
    let session = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request_id(&session),"result":{"thread":{"id":"thread-1"}}})),
            &Mapper,
        )
        .unwrap();
    running_turn(&mut runtime);
    let steer = runtime
        .command(AdapterCommand::Steer {
            prompt: Prompt::new("prompt", "Change direction").unwrap(),
            client_message_id: RequiredText::new("client", "steer-1").unwrap(),
        })
        .expect("active turn steer");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(steer.frame().payload()).unwrap(),
        json!({"id":request_id(&steer),"method":"turn/steer","params":{
            "threadId":"thread-1","expectedTurnId":"turn-1",
            "clientUserMessageId":"steer-1",
            "input":[{"type":"text","text":"Change direction"}]
        }})
    );
    let accepted = runtime
        .ingest(
            &frame(json!({"id":request_id(&steer),"result":{"turnId":"turn-1"}})),
            &Mapper,
        )
        .expect("steer response");
    assert!(matches!(
        accepted.events(),
        [AdapterEvent::SteerResponse { request_id: id, accepted: true }]
            if *id == request_id(&steer)
    ));
    let rejected = runtime
        .command(AdapterCommand::Steer {
            prompt: Prompt::new("prompt", "Rejected direction").unwrap(),
            client_message_id: RequiredText::new("client", "steer-rejected").unwrap(),
        })
        .expect("second steer request");
    let refusal = runtime
        .ingest(
            &frame(
                json!({"id":request_id(&rejected),"error":{"code":-32602,"message":"stale turn"}}),
            ),
            &Mapper,
        )
        .expect("asynchronous steer rejection");
    assert!(refusal.events().iter().any(|event| matches!(
        event,
        AdapterEvent::SteerResponse { request_id: id, accepted: false }
            if *id == request_id(&rejected)
    )));
    runtime
        .ingest(
            &frame(json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(
        runtime
            .command(AdapterCommand::Steer {
                prompt: Prompt::new("prompt", "Too late").unwrap(),
                client_message_id: RequiredText::new("client", "steer-2").unwrap(),
            })
            .is_err()
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn dynamic_browse_call_is_answered_without_a_synthetic_transcript_event() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({"id":"browse-1","method":"item/tool/call","params":{
                "threadId":"thread-1","turnId":"turn-1","callId":"call-1",
                "tool":"sage_browse","arguments":{"url":"http://127.0.0.1:43124/proof"}
            }})),
            &Mapper,
        )
        .expect("admitted dynamic browse call");
    assert!(batch.events().is_empty());
    assert_eq!(batch.egress().len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(batch.egress()[0].frame().payload()).unwrap(),
        json!({"id":"browse-1","result":{
            "success":true,
            "contentItems":[{"type":"inputText","text":"http://127.0.0.1:43124/proof"}]
        }})
    );
}

#[test]
fn notification_first_turn_start_ignores_the_late_response_state_regression() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: RequiredText::new("client", "notification-first").unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let id = request_id(&egress);
    let running = runtime
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"thread-1","turn":{"id":"turn-notification-first"}
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        running.events(),
        [AdapterEvent::Turn {
            state: crate::acp::lifecycle::TurnState::Running,
            ..
        }]
    ));
    let late = runtime
        .ingest(
            &frame(json!({"id":id,"result":{"turn":{"id":"turn-notification-first"}}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(late.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn completed_notification_first_turn_admits_the_next_turn_started() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);

    let first = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Run one bounded command").unwrap(),
            client_message_id: RequiredText::new("client", "first-command").unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let first_id = request_id(&first);
    runtime
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
            }})),
            &Mapper,
        )
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}
            }})),
            &Mapper,
        )
        .unwrap();
    let late_first_response = runtime
        .ingest(
            &frame(json!({"id":first_id,"result":{"turn":{"id":"turn-1"}}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        late_first_response.events(),
        [AdapterEvent::IgnoredDuplicate]
    ));

    let second = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Follow up without editing").unwrap(),
            client_message_id: RequiredText::new("client", "second-follow-up").unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let second_id = request_id(&second);
    let started = runtime
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"thread-1","turn":{"id":"turn-2","status":"inProgress"}
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        started.events(),
        [AdapterEvent::Turn {
            state: crate::acp::lifecycle::TurnState::Running,
            ..
        }]
    ));
    let late_second_response = runtime
        .ingest(
            &frame(json!({"id":second_id,"result":{"turn":{"id":"turn-2"}}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        late_second_response.events(),
        [AdapterEvent::IgnoredLate]
    ));

    for invalid in [
        json!({"method":"turn/started","params":{
            "threadId":"foreign-thread","turn":{"id":"turn-2","status":"inProgress"}
        }}),
        json!({"method":"turn/started","params":{
            "threadId":"thread-1","turn":{"id":"unexpected-turn","status":"inProgress"}
        }}),
    ] {
        assert!(matches!(
            runtime.ingest(&frame(invalid), &Mapper),
            Err(AdapterRuntimeError::Operation)
        ));
    }
}

#[test]
fn learned_child_thread_turn_start_does_not_reject_parent_session() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    runtime
        .ingest(
            &frame(json!({"method":"item/completed","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"spawn-1","type":"collabAgentToolCall","status":"completed",
                    "tool":"spawnAgent","senderThreadId":"thread-1",
                    "receiverThreadIds":["child-1"],
                    "agentsStates":{"child-1":{"status":"running"}}
                }
            }})),
            &Mapper,
        )
        .expect("spawn item");

    let child = runtime.ingest(
        &frame(json!({"method":"turn/started","params":{
            "threadId":"child-1","turn":{"id":"child-turn-1","status":"inProgress"}
        }})),
        &Mapper,
    );
    assert!(matches!(
        child.unwrap().events(),
        [AdapterEvent::IgnoredLate]
    ));
    runtime
        .ingest(
            &frame(json!({"method":"item/completed","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"activity-1","type":"subAgentActivity",
                    "agentThreadId":"child-2","agentPath":"Worker","kind":"started"
                }
            }})),
            &Mapper,
        )
        .expect("subagent activity");
    let activity_child = runtime
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"child-2","turn":{"id":"child-turn-2","status":"inProgress"}
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        activity_child.events(),
        [AdapterEvent::IgnoredLate]
    ));
    let child_thread = runtime
        .ingest(
            &frame(json!({"method":"thread/started","params":{
                "thread":{"id":"child-2"}
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(child_thread.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn native_child_identity_parent_activity_stays_separate_from_display() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({"method":"item/completed","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"activity-native","type":"subAgentActivity",
                    "agentThreadId":"child-native","agentPath":"Worker","kind":"started"
                }
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(batch.events(),
        [AdapterEvent::ChildThreadItem { id, child_thread_id, payload: NormalizedPayload::Collaboration { agent_key: Some(display), .. }, .. }]
            if id.external_id.as_str() == "activity-native"
                && child_thread_id.as_str() == "child-native"
                && display.as_str() == "child-native"
    ));
}

#[test]
fn native_child_identity_terminal_only_activity_is_admitted_from_exact_parent() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({"method":"item/completed","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"activity-terminal","type":"subAgentActivity",
                    "agentThreadId":"child-terminal","agentPath":"Worker","kind":"completed"
                }
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(batch.events(),
        [AdapterEvent::ChildThreadItem { lifecycle: ItemLifecycle::Completed, child_thread_id, .. }]
            if child_thread_id.as_str() == "child-terminal"
    ));
    assert!(matches!(
        runtime.ingest(
            &frame(json!({"method":"item/completed","params":{
                "threadId":"foreign-thread","turnId":"turn-1","item":{
                    "id":"activity-foreign","type":"subAgentActivity",
                    "agentThreadId":"child-foreign","agentPath":"Worker","kind":"completed"
                }
            }})),
            &Mapper,
        ),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn synthetic_child_command_notification_is_retained_without_a_parent_item() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    runtime.enable_observed_goal_for_test();
    running_turn(&mut runtime);
    runtime
        .ingest(
            &frame(json!({"method":"item/completed","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"activity-native","type":"subAgentActivity",
                    "agentThreadId":"child-native","agentPath":"Worker","kind":"started"
                }
            }})),
            &Mapper,
        )
        .unwrap();
    let child = runtime
        .ingest(
            &frame(json!({"method":"item/completed","params":{
                "threadId":"child-native","turnId":"child-turn-1","item":{
                    "id":"command-1","type":"commandExecution","status":"completed",
                    "command":"rg TODO src","aggregatedOutput":"done","exitCode":0
                }
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(child.events(),
        [AdapterEvent::ChildLiveItem { child_thread_id, child_turn_id, item_id, payload: NormalizedPayload::ToolCall { .. }, .. }]
            if child_thread_id.as_str() == "child-native"
                && child_turn_id.as_str() == "child-turn-1"
                && item_id.as_str() == "command-1"
    ));
}

#[test]
fn oldest_child_notification_after_more_than_32_children_keeps_parent_alive() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    for index in 1..=34 {
        runtime
            .ingest(
                &frame(json!({"method":"item/completed","params":{
                    "threadId":"thread-1","turnId":"turn-1","item":{
                        "id":format!("activity-{index}"),"type":"subAgentActivity",
                        "agentThreadId":format!("child-{index}"),
                        "agentPath":"Worker","kind":"started"
                    }
                }})),
                &Mapper,
            )
            .expect("child activity");
    }
    let oldest = runtime
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"child-1","turn":{"id":"child-turn-1","status":"inProgress"}
            }})),
            &Mapper,
        )
        .expect("oldest child must not tear down the parent");
    assert!(matches!(oldest.events(), [AdapterEvent::IgnoredLate]));
    assert!(matches!(
        runtime.ingest(&frame(json!({"id":999,"result":{}})), &Mapper),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn prompt_and_interrupt_rpc_errors_emit_terminal_failure_without_replay() {
    let mut prompt_failed = fixture_runtime();
    ready(&mut prompt_failed);
    let prompt = prompt_failed
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Never replay").unwrap(),
            client_message_id: RequiredText::new("client", "failed-prompt").unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt_id = request_id(&prompt);
    let failed = prompt_failed
        .ingest(
            &frame(json!({"id":prompt_id,"error":{"message":"rejected"}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        failed.events(),
        [
            AdapterEvent::Error(_),
            AdapterEvent::Turn {
                state: crate::acp::lifecycle::TurnState::Failed,
                ..
            }
        ]
    ));
    assert!(prompt_failed.command(AdapterCommand::Cancel).is_err());

    let mut interrupt_failed = fixture_runtime();
    ready(&mut interrupt_failed);
    running_turn(&mut interrupt_failed);
    let cancel = interrupt_failed.command(AdapterCommand::Cancel).unwrap();
    let cancel_id = request_id(&cancel);
    let failed = interrupt_failed
        .ingest(
            &frame(json!({"id":cancel_id,"error":{"message":"interrupt failed"}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        failed.events(),
        [
            AdapterEvent::Error(_),
            AdapterEvent::Turn {
                state: crate::acp::lifecycle::TurnState::Failed,
                ..
            }
        ]
    ));
}

#[test]
fn launch_stays_unavailable_without_exact_artifact_admission() {
    let packet = parse_packet_metadata(include_bytes!(
        "../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .expect("packet metadata");
    let policy = parse_security_policy(
        include_bytes!("../../../../../testdata/acp/codex/security-policy-v1.json"),
        &packet,
    )
    .expect("security policy prerequisite");
    let reproduction =
        crate::acp::bindings::codex::reproduction::ReproducedPackagePrerequisite::fixture(&packet);
    let managed_staging = crate::acp::bindings::codex::managed::ManagedStagingPrerequisite::fixture(
        &packet,
        &reproduction,
    );
    assert_eq!(
        CodexRuntime::from_metadata_packet(
            &packet,
            &policy,
            &reproduction,
            &managed_staging,
            [1; 32],
            RuntimeEpoch::new(1).expect("epoch"),
            SageAgentSessionId::parse("session-launch").expect("session")
        )
        .err(),
        Some(AdapterRuntimeError::LaunchUnavailable)
    );
}

#[cfg(windows)]
#[test]
fn admitted_binding_is_the_only_constructor_that_crosses_the_launch_gate() {
    let registration = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let binding = registration.refresh().unwrap();

    assert!(
        CodexRuntime::from_admitted_binding(
            binding,
            launch_root(
                launch_project_id(),
                ProjectGeneration::ZERO,
                launch_root_id(),
            ),
            RuntimeEpoch::new(1).expect("epoch"),
            SageAgentSessionId::parse("session-admitted").expect("session"),
        )
        .is_ok()
    );
}

#[cfg(windows)]
#[test]
fn observed_cli_binding_accepts_its_own_initialize_identity() {
    use std::{fs, sync::Arc};

    let path = observed_cli_path("sage-observed-codex-runtime");
    let binding = crate::acp::bindings::codex::AdmittedCodexBinding::admit_observed_for_test(
        path.clone(),
        version_line("codex"),
    )
    .unwrap();
    assert_ne!(binding.binding_digest_hex(), binding.artifact_digest());
    let mut runtime = CodexRuntime::from_admitted_binding(
        Arc::new(binding),
        launch_root(
            launch_project_id(),
            ProjectGeneration::ZERO,
            launch_root_id(),
        ),
        RuntimeEpoch::new(22).unwrap(),
        SageAgentSessionId::parse("session-observed-cli").unwrap(),
    )
    .unwrap();
    let egress = runtime.command(AdapterCommand::Initialize).unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();

    let initialized = runtime.ingest(
        &frame(json!({"id": id, "result": {
            "userAgent": user_agent("Codex Desktop", "dumb"),
            "codexHome": r"C:\Users\test\.codex",
            "platformFamily": "windows",
            "platformOs": "windows"
        }})),
        &Mapper,
    );

    assert!(matches!(
        initialized.unwrap().events(),
        [AdapterEvent::Initialized { .. }]
    ));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[cfg(windows)]
#[test]
fn an_observed_pinned_binding_publishes_its_own_identity_not_the_pinned_package() {
    use std::{fs, sync::Arc};

    let path = observed_cli_path("sage-observed-codex-identity");
    let binding = crate::acp::bindings::codex::AdmittedCodexBinding::admit_observed_for_test(
        path.clone(),
        version_line("codex-cli"),
    )
    .unwrap();

    let admitted = binding.capability_ceiling().unwrap();
    assert_eq!(
        admitted.protocol_version.as_str(),
        OBSERVED_PROTOCOL_IDENTITY
    );
    assert_ne!(admitted.protocol_version.as_str(), PINNED_PROTOCOL_IDENTITY);

    let mut runtime = CodexRuntime::from_admitted_binding(
        Arc::new(binding),
        launch_root(
            launch_project_id(),
            ProjectGeneration::ZERO,
            launch_root_id(),
        ),
        RuntimeEpoch::new(23).unwrap(),
        SageAgentSessionId::parse("session-observed-identity").unwrap(),
    )
    .unwrap();
    let egress = runtime.command(AdapterCommand::Initialize).unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();

    let initialized = runtime
        .ingest(
            &frame(json!({"id": id, "result": {
                "userAgent": user_agent("Codex Desktop", "dumb"),
                "codexHome": r"C:\Users\test\.codex",
                "platformFamily": "windows",
                "platformOs": "windows"
            }})),
            &Mapper,
        )
        .unwrap();

    let [AdapterEvent::Initialized { capabilities }] = initialized.events() else {
        panic!("observed initialize publishes a capability snapshot");
    };
    assert_eq!(
        capabilities.protocol_version.as_str(),
        OBSERVED_PROTOCOL_IDENTITY
    );
    assert!(capabilities.new_session && capabilities.prompt && capabilities.cancel);
    assert!(!capabilities.load_session);
    assert!(matches!(
        runtime.command(AdapterCommand::LoadSession {
            external_session_id: ExternalId::new("external", "thread-1").unwrap(),
        }),
        Err(AdapterRuntimeError::Operation)
    ));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn observed_remote_control_status_notification_is_ignored_during_initialization() {
    let mut runtime = fixture_runtime();
    let notification = runtime
        .ingest(
            &frame(json!({
                "method": "remoteControl/status/changed",
                "params": {
                    "environmentId": null,
                    "installationId": "installation-observed",
                    "serverName": "windows-host",
                    "status": "disabled"
                }
            })),
            &Mapper,
        )
        .expect("recognized operational notification");

    assert!(matches!(notification.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn observed_idle_thread_status_notification_is_ignored_during_ready_handoff() {
    let mut runtime = fixture_runtime();
    let notification = runtime
        .ingest(
            &frame(json!({
                "emittedAtMs": 1_788_261_366_663_u64,
                "method": "thread/status/changed",
                "params": {
                    "status": { "type": "idle" },
                    "threadId": "thread-observed"
                }
            })),
            &Mapper,
        )
        .expect("observed idle notification is informational");

    assert!(matches!(notification.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn observed_active_thread_status_notification_is_ignored_before_turn_started() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").expect("prompt"),
            client_message_id: RequiredText::new("client", "active-status").expect("client"),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .expect("prompt");
    let notification = runtime
        .ingest(
            &frame(json!({
                "method": "thread/status/changed",
                "params": {
                    "status": { "type": "active", "activeFlags": [] },
                    "threadId": "thread-observed"
                }
            })),
            &Mapper,
        )
        .expect("observed active notification is informational before turn start");

    assert!(matches!(notification.events(), [AdapterEvent::IgnoredLate]));
    let started = runtime
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"thread-1","turn":{"id":"turn-active-status"}
            }})),
            &Mapper,
        )
        .expect("turn start follows active status");
    assert!(matches!(
        started.events(),
        [AdapterEvent::Turn {
            state: crate::acp::lifecycle::TurnState::Running,
            ..
        }]
    ));
}

#[test]
fn observed_system_error_thread_status_fails_the_pending_turn_without_tearing_down_the_session() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Reply with PONG").expect("prompt"),
            client_message_id: RequiredText::new("client", "system-error").expect("client"),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .expect("prompt");

    let notification = runtime
        .ingest(
            &frame(json!({
                "emittedAtMs": 1_790_173_669_835_u64,
                "method": "thread/status/changed",
                "params": {
                    "status": { "type": "systemError" },
                    "threadId": "thread-1"
                }
            })),
            &Mapper,
        )
        .expect("system errors end the pending turn");

    assert!(matches!(
        notification.events(),
        [
            AdapterEvent::Error(_),
            AdapterEvent::Turn {
                state: crate::acp::lifecycle::TurnState::Failed,
                ..
            }
        ]
    ));
    assert!(
        runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "Retry with PONG").expect("prompt"),
                client_message_id: RequiredText::new("client", "system-error-retry")
                    .expect("client"),
                access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .is_ok()
    );
}

#[test]
fn observed_goal_cleared_notification_is_ignored_during_ready_handoff() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let notification = runtime
        .ingest(
            &frame(json!({
                "method": "thread/goal/cleared",
                "params": { "threadId": "thread-1" }
            })),
            &Mapper,
        )
        .expect("observed goal clear is informational during handoff");

    assert!(matches!(notification.events(), [AdapterEvent::IgnoredLate]));
}

#[cfg(windows)]
#[test]
fn admitted_observed_codex_hydrates_goal_and_bounds_existing_mutations() {
    use std::{fs, sync::Arc};

    let path = observed_cli_path("sage-observed-goal");
    let binding = crate::acp::bindings::codex::AdmittedCodexBinding::admit_observed_for_test(
        path.clone(),
        version_line("codex-cli"),
    )
    .unwrap();
    let mut runtime = CodexRuntime::from_admitted_binding(
        Arc::new(binding),
        launch_root(
            launch_project_id(),
            ProjectGeneration::ZERO,
            launch_root_id(),
        ),
        RuntimeEpoch::new(24).unwrap(),
        SageAgentSessionId::parse("session-observed-goal").unwrap(),
    )
    .unwrap();
    let initialize = runtime.command(AdapterCommand::Initialize).unwrap();
    let initialize_id = request_id(&initialize);
    runtime
        .ingest(
            &frame(json!({"id":initialize_id,"result":{
                "userAgent":user_agent("Codex Desktop", "dumb"),
                "codexHome":r"C:\Users\test\.codex",
                "platformFamily":"windows","platformOs":"windows"
            }})),
            &Mapper,
        )
        .unwrap();
    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    let ready = runtime
        .ingest(
            &frame(json!({"id":request_id(&start),"result":{
                "thread":{"id":"thread-goal"}
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        ready.events(),
        [AdapterEvent::SessionReady { .. }]
    ));
    assert!(ready.egress().is_empty());
    let egress = runtime.goal_get().unwrap().expect("observed goal get");
    let get: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(get["method"], "thread/goal/get");
    assert_eq!(get["params"]["threadId"], "thread-goal");
    runtime
        .ingest(
            &frame(json!({"id":get["id"],"result":{"goal":{
                "objective":"Ship safely","status":"active", "tokenBudget":1000,
                "tokensUsed":42,"timeUsedSeconds":125
            }}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        runtime.goal_state().unwrap().objective(),
        Some("Ship safely")
    );
    let pause = runtime
        .mutate_goal(crate::acp::goal::GoalAction::PauseExisting)
        .unwrap();
    let pause_wire: Value = serde_json::from_slice(pause.frame().payload()).unwrap();
    assert_eq!(pause_wire["method"], "thread/goal/set");
    assert_eq!(
        pause_wire["params"],
        json!({"threadId":"thread-goal","status":"paused"})
    );
    runtime
        .ingest(
            &frame(json!({"id":pause_wire["id"],"result":{"goal":{
                "objective":"Ship safely","status":"paused","tokenBudget":1000,
                "tokensUsed":42,"timeUsedSeconds":125
            }}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(crate::acp::goal::NativeRpcReply::GoalApplied)
    );
    assert_eq!(runtime.take_native_rpc_reply(), None);
    let clear = runtime
        .mutate_goal(crate::acp::goal::GoalAction::ClearExisting)
        .unwrap();
    let clear_wire: Value = serde_json::from_slice(clear.frame().payload()).unwrap();
    assert_eq!(clear_wire["method"], "thread/goal/clear");
    assert_eq!(clear_wire["params"], json!({"threadId":"thread-goal"}));
    assert_eq!(
        runtime
            .ingest(
                &frame(json!({"id":clear_wire["id"],"result":{"cleared":false}})),
                &Mapper
            )
            .err(),
        Some(AdapterRuntimeError::GoalMutationRejected)
    );
    assert_eq!(
        runtime.goal_state().unwrap().objective(),
        Some("Ship safely")
    );
    let clear = runtime
        .mutate_goal(crate::acp::goal::GoalAction::ClearExisting)
        .unwrap();
    let clear_wire: Value = serde_json::from_slice(clear.frame().payload()).unwrap();
    assert_eq!(
        runtime
            .ingest(
                &frame(json!({"id":clear_wire["id"],"error":{"code":-1,"message":"denied"}})),
                &Mapper
            )
            .err(),
        Some(AdapterRuntimeError::GoalMutationRejected)
    );
    assert_eq!(
        runtime.goal_state().unwrap().objective(),
        Some("Ship safely")
    );
    let clear = runtime
        .mutate_goal(crate::acp::goal::GoalAction::ClearExisting)
        .unwrap();
    runtime
        .resolve_egress(clear.into_token(), EgressResolution::DeliveryUnknown)
        .unwrap();
    assert_eq!(runtime.goal_state().unwrap().availability(), "unavailable");
    assert!(
        runtime
            .mutate_goal(crate::acp::goal::GoalAction::ClearExisting)
            .is_err()
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn goal_updates_are_ignored_for_the_live_thread_and_rejected_for_a_foreign_one() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let notification = runtime
        .ingest(
            &frame(json!({
                "method": "thread/goal/updated",
                "params": { "threadId": "thread-1", "goal": {} }
            })),
            &Mapper,
        )
        .expect("Codex-owned goal state must not tear the runtime down");

    assert!(matches!(notification.events(), [AdapterEvent::IgnoredLate]));
    assert!(matches!(
        runtime.ingest(
            &frame(json!({
                "method": "thread/goal/updated",
                "params": { "threadId": "thread-foreign", "goal": {} }
            })),
            &Mapper,
        ),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn unrecognized_thread_status_notification_is_rejected() {
    let mut runtime = fixture_runtime();
    let notification = runtime.ingest(
        &frame(json!({
            "method": "thread/status/changed",
            "params": {
                "status": { "type": "running" },
                "threadId": "thread-observed"
            }
        })),
        &Mapper,
    );

    assert!(matches!(notification, Err(AdapterRuntimeError::Frame)));
}

#[test]
fn historical_usage_replayed_during_resume_is_ignored_without_an_active_turn() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let usage = runtime
        .ingest(
            &frame(json!({
                "method": "thread/tokenUsage/updated",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "historical-turn",
                    "tokenUsage": { "last": { "inputTokens": 8, "outputTokens": 3 } }
                }
            })),
            &Mapper,
        )
        .expect("bounded historical usage is informational during resume");

    assert!(matches!(usage.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn observed_mcp_startup_status_notification_is_ignored_during_activation() {
    let mut runtime = fixture_runtime();
    let notification = runtime
        .ingest(
            &frame(json!({
                "method": "mcpServer/startupStatus/updated",
                "params": {
                    "error": null,
                    "failureReason": null,
                    "name": "developer",
                    "status": "starting",
                    "threadId": "thread-observed"
                }
            })),
            &Mapper,
        )
        .expect("recognized operational notification");

    assert!(matches!(notification.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn observed_mcp_ready_status_notification_is_ignored_during_turn_start() {
    let mut runtime = fixture_runtime();
    let notification = runtime
        .ingest(
            &frame(json!({
                "emittedAtMs": 1_787_930_996_883_u64,
                "method": "mcpServer/startupStatus/updated",
                "params": {
                    "error": null,
                    "failureReason": null,
                    "name": "developer",
                    "status": "ready",
                    "threadId": "f12d7062-15e0-4d7b-94a2-8a52382a4611"
                }
            })),
            &Mapper,
        )
        .expect("observed operational ready notification");

    assert!(matches!(notification.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn observed_mcp_cancelled_status_notification_is_ignored_during_activation() {
    let mut runtime = fixture_runtime();
    let notification = runtime.ingest(
        &frame(json!({
            "method": "mcpServer/startupStatus/updated",
            "params": {
                "error": null,
                "failureReason": null,
                "name": "developer",
                "status": "cancelled",
                "threadId": "thread-observed"
            }
        })),
        &Mapper,
    );

    assert!(notification.is_ok());
}

#[test]
fn failed_optional_browser_mcp_does_not_abort_chat_activation() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let request = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({
                "method": "mcpServer/startupStatus/updated",
                "params": {
                    "error": "Stale browser helper failed",
                    "failureReason": null,
                    "name": "sage-browser",
                    "status": "failed",
                    "threadId": "foreign-thread"
                }
            })),
            &Mapper,
        )
        .expect("pre-bind foreign failure remains bounded");
    let notification = runtime.ingest(
        &frame(json!({
            "method": "mcpServer/startupStatus/updated",
            "params": {
                "error": "MCP client for `sage-browser` failed to start",
                "failureReason": null,
                "name": "sage-browser",
                "status": "failed",
                "threadId": "thread-observed"
            }
        })),
        &Mapper,
    );

    assert!(
        notification.is_ok(),
        "optional browser failure must not fail activation"
    );
    let started = runtime
        .ingest(
            &frame(json!({"method":"thread/started","params":{
                "thread":{"id":"thread-observed"}
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(started.events(), [AdapterEvent::IgnoredLate]));
    let started = runtime
        .ingest(
            &frame(json!({"id":request_id(&request),"result":{"thread":{"id":"thread-observed"}}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        started.events(),
        [AdapterEvent::SessionReady { .. }, AdapterEvent::Error(NormalizedPayload::Error { code, .. })]
            if code.as_str() == "browserMcpUnavailable"
    ));
    let repeated = runtime
        .ingest(
            &frame(json!({
                "method": "mcpServer/startupStatus/updated",
                "params": {
                    "error": "MCP client for `sage-browser` failed to start",
                    "failureReason": null,
                    "name": "sage-browser",
                    "status": "failed",
                    "threadId": "thread-observed"
                }
            })),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(repeated.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn unknown_mcp_startup_status_fails_closed() {
    let mut runtime = fixture_runtime();

    assert!(
        runtime
            .ingest(
                &frame(json!({
                    "method": "mcpServer/startupStatus/updated",
                    "params": {
                        "error": null,
                        "failureReason": null,
                        "name": "developer",
                        "status": "unknown",
                        "threadId": "thread-observed"
                    }
                })),
                &Mapper,
            )
            .is_err()
    );
}

#[cfg(windows)]
#[test]
fn runtime_retains_admission_authority_and_frozen_environment_until_teardown() {
    let registration = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let binding = registration.refresh().unwrap();
    let expected_environment = binding.environment().entries().to_vec();
    let retained = std::sync::Arc::downgrade(&binding);
    let runtime = CodexRuntime::from_admitted_binding(
        binding,
        launch_root(
            launch_project_id(),
            ProjectGeneration::ZERO,
            launch_root_id(),
        ),
        RuntimeEpoch::new(1).expect("epoch"),
        SageAgentSessionId::parse("session-retained-authority").expect("session"),
    )
    .unwrap();

    drop(registration);
    assert!(retained.upgrade().is_some());
    assert_eq!(runtime.launch_environment().unwrap(), expected_environment);
    drop(runtime);
    assert!(retained.upgrade().is_none());
}

#[cfg(windows)]
#[test]
fn admitted_runtime_builds_the_exact_contained_app_server_launch_contract() {
    use std::ffi::OsStr;

    use crate::acp::process::RequiredContainment;

    let registration = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let binding = registration.refresh().unwrap();
    let expected_entrypoint = binding.entrypoint().path.clone();
    let expected_sha256 = binding.entrypoint().sha256;
    let expected_bytes = binding.entrypoint().byte_len;
    let expected_environment = binding.environment().entries().to_vec();
    let working_directory = std::env::current_dir().unwrap().canonicalize().unwrap();
    let mut runtime = CodexRuntime::from_admitted_binding(
        binding,
        launch_root(
            launch_project_id(),
            ProjectGeneration::ZERO,
            launch_root_id(),
        ),
        RuntimeEpoch::new(23).expect("epoch"),
        SageAgentSessionId::parse("session-launch-contract").expect("session"),
    )
    .unwrap();

    let spec = runtime.launch_spec().expect("admitted launch contract");

    assert_eq!(spec.runtime_epoch, 23);
    assert!(!spec.binding_id.is_nil());
    assert!(!spec.artifact.artifact_id.is_nil());
    assert_ne!(spec.binding_id, spec.artifact.artifact_id);
    assert_eq!(spec.artifact.canonical_absolute_path, expected_entrypoint);
    assert_eq!(spec.artifact.sha256, expected_sha256);
    assert_eq!(spec.artifact.byte_len, expected_bytes);
    assert_eq!(spec.fixed_args.as_slice(), [OsStr::new("app-server")]);
    assert_eq!(
        spec.environment
            .as_slice()
            .iter()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.to_string_lossy().into_owned(),
                )
            })
            .collect::<Vec<_>>(),
        expected_environment
    );
    assert_eq!(
        spec.environment
            .as_slice()
            .iter()
            .map(|(key, _)| key.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        [
            "CODEX_HOME",
            "PATH",
            "ComSpec",
            "PATHEXT",
            "SystemRoot",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "APPDATA",
            "WINDIR",
        ]
    );
    let path = spec
        .environment
        .as_slice()
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(OsStr::new("PATH")))
        .map(|(_, value)| value)
        .expect("the npm Codex launcher needs an explicit node PATH");
    assert!(
        std::path::Path::new(path).join("node.exe").is_file(),
        "the isolated environment admits the node runtime resolved from the launcher"
    );
    for inherited in ["HOME", "OPENAI_API_KEY"] {
        assert!(
            spec.environment
                .as_slice()
                .iter()
                .all(|(key, _)| !key.eq_ignore_ascii_case(OsStr::new(inherited)))
        );
    }
    assert_eq!(spec.project_root.as_path(), working_directory);
    assert_eq!(spec.required_guarantee, RequiredContainment::EnforcedTree);
    assert_eq!(
        spec.limits.active_processes,
        spec.limits.descendants.saturating_add(1)
    );
}

#[test]
fn metadata_only_runtime_cannot_build_a_process_launch_contract() {
    let mut runtime = fixture_runtime();

    assert_eq!(
        runtime.launch_spec().err(),
        Some(AdapterRuntimeError::LaunchUnavailable)
    );
}

#[cfg(windows)]
#[test]
fn runtime_launch_uses_the_exact_consumed_root_when_same_ids_name_another_identity() {
    let base =
        std::env::temp_dir().join(format!("sage-codex-root-identity-{}", uuid::Uuid::new_v4()));
    let exact = base.join("exact");
    let substitute = base.join("substitute");
    std::fs::create_dir_all(&exact).unwrap();
    std::fs::create_dir_all(&substitute).unwrap();
    let registration = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let binding = registration.refresh().unwrap();
    let project_id = launch_project_id();
    let root_id = launch_root_id();
    let exact_authority = CodexProjectRootAuthority::for_test(
        ProjectRootAuthority::for_test(
            project_id.clone(),
            ProjectGeneration::ZERO,
            root_id.clone(),
            &exact,
        )
        .unwrap(),
    );
    let substitute_authority =
        ProjectRootAuthority::for_test(project_id, ProjectGeneration::ZERO, root_id, &substitute)
            .unwrap();
    let mut runtime = CodexRuntime::from_admitted_binding(
        binding,
        exact_authority,
        RuntimeEpoch::new(24).expect("epoch"),
        SageAgentSessionId::parse("session-root-identity").expect("session"),
    )
    .unwrap();

    let spec = runtime.launch_spec().unwrap();

    assert_eq!(spec.project_root.as_path(), exact.canonicalize().unwrap());
    assert_ne!(spec.project_root.as_path(), substitute_authority.as_path());
    assert_eq!(
        runtime.launch_spec().err(),
        Some(AdapterRuntimeError::LaunchUnavailable)
    );
    drop(spec);
    drop(substitute_authority);
    let _ = std::fs::remove_dir_all(base);
}

#[cfg(windows)]
#[test]
fn retained_launch_authority_fails_closed_after_binding_drift() {
    let registration = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let binding = registration.refresh().unwrap();
    let mut runtime = CodexRuntime::from_admitted_binding(
        binding,
        launch_root(
            launch_project_id(),
            ProjectGeneration::ZERO,
            launch_root_id(),
        ),
        RuntimeEpoch::new(25).expect("epoch"),
        SageAgentSessionId::parse("session-launch-drift").expect("session"),
    )
    .unwrap();

    registration.induce_drift_for_test();
    assert_eq!(
        runtime.launch_spec().err(),
        Some(AdapterRuntimeError::LaunchUnavailable)
    );
    assert_eq!(
        registration.refresh().err(),
        Some(crate::acp::bindings::codex::CodexAdmissionUnavailable::Drift)
    );
    assert!(!registration.is_available());
}

#[test]
fn initialize_session_prompt_stream_tool_usage_cancel_and_terminal_are_normalized() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    assert!(runtime.capabilities().expect("capabilities").prompt);
    running_turn(&mut runtime);

    let text = runtime
        .ingest(
            &frame(json!({"method":"item/agentMessage/delta","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"message-1",
                "delta":"read C:\\Users\\private\\token with Bearer secret-value"
            }})),
            &Mapper,
        )
        .expect("text delta");
    let AdapterEvent::Item { payload, .. } = &text.events()[0] else {
        panic!("item event")
    };
    let encoded = serde_json::to_string(payload).expect("safe payload");
    assert!(!encoded.contains("Users"));
    assert!(!encoded.contains("secret-value"));

    let tool = runtime
        .ingest(
            &frame(json!({"method":"item/completed","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"tool-1","type":"commandExecution","status":"completed",
                    "command":"do not expose this"
                }
            }})),
            &Mapper,
        )
        .expect("tool terminal");
    assert!(matches!(
        tool.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { .. },
            ..
        }]
    ));

    let usage = runtime
        .ingest(
            &frame(json!({"method":"thread/tokenUsage/updated","params":{
                "threadId":"thread-1","turnId":"turn-1",
                "tokenUsage":{"last":{"inputTokens":8,"outputTokens":3}}
            }})),
            &Mapper,
        )
        .expect("usage");
    let [AdapterEvent::Usage { turn_id, payload }] = usage.events() else {
        panic!("usage event")
    };
    assert_eq!(
        turn_id.turn_id.as_ref().map(ExternalId::as_str),
        Some("turn-1")
    );
    assert!(turn_id.turn_client_message_id.is_some());
    assert_eq!(
        payload,
        &NormalizedPayload::Usage {
            input_tokens: 8,
            output_tokens: 3,
            context_window: None,
        }
    );

    let cancel = runtime.command(AdapterCommand::Cancel).expect("cancel");
    assert!(cancel.token().is_control() && cancel.token().is_cancel());
    let cancel_id = request_id(&cancel);
    assert_eq!(
        serde_json::from_slice::<Value>(cancel.frame().payload()).expect("cancel json")["method"],
        "turn/interrupt"
    );
    assert!(matches!(
        runtime
            .ingest(&frame(json!({"id":cancel_id,"result":{}})), &Mapper)
            .expect("cancel response")
            .events(),
        [AdapterEvent::Acknowledged]
    ));
    let terminal = runtime
        .ingest(
            &frame(json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"interrupted"}
            }})),
            &Mapper,
        )
        .expect("terminal");
    assert!(
        matches!(
            terminal.events(),
            [AdapterEvent::Turn {
                state: crate::acp::lifecycle::TurnState::Cancelled,
                ..
            }]
        ),
        "Sage asked for this stop, so it is a user cancel"
    );
}

#[test]
fn sparse_command_updates_preserve_and_explicitly_clear_typed_fields_on_one_item() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);

    let initial = runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-1","type":"commandExecution","status":"inProgress",
                    "aggregatedOutput":"\"Bearer\",  \t\r\nopaque-command-credential path=C:\\private file:///secret",
                    "commandActions":[{
                        "type":"read","command":"cat src/lib.rs","name":"read source",
                        "path":"C:\\repo\\src\\lib.rs"
                    }]
                }
            }})),
            &Mapper,
        )
        .expect("initial command");
    let AdapterEvent::Item {
        id: first_id,
        payload,
        ..
    } = &initial.events()[0]
    else {
        panic!("initial command item")
    };
    assert!(matches!(
        payload,
        NormalizedPayload::ToolCall {
            output_summary: Some(output),
            locations,
            ..
        } if !output.as_str().contains("opaque-command-credential")
            && !output.as_str().contains("private")
            && !output.as_str().contains("file:///")
            && locations.as_slice().len() == 1
    ));
    let patched = runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-1","type":"commandExecution","status":"inProgress",
                    "aggregatedOutput":"\"Bearer:\",  \t\r\nopaque-command-credential path=C:\\private file:///secret"
                }
            }})),
            &Mapper,
        )
        .expect("same-status command patch");
    let AdapterEvent::Item {
        id: patched_id,
        payload,
        ..
    } = &patched.events()[0]
    else {
        panic!("patched command item")
    };
    assert_eq!(
        first_id, patched_id,
        "same external command remains one item"
    );
    assert!(matches!(
        payload,
        NormalizedPayload::ToolCall {
            output_summary: Some(output),
            locations,
            ..
        } if !output.as_str().contains("opaque-command-credential")
            && !output.as_str().contains("private")
            && !output.as_str().contains("file:///")
            && locations.as_slice().len() == 1
    ));
    let preserved = runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-1","type":"commandExecution","status":"inProgress"
                }
            }})),
            &Mapper,
        )
        .expect("sparse command");
    let AdapterEvent::Item { payload, .. } = &preserved.events()[0] else {
        panic!("preserved command item")
    };
    assert!(matches!(
        payload,
        NormalizedPayload::ToolCall {
            output_summary: Some(output),
            locations,
            ..
        } if !output.as_str().contains("opaque-command-credential")
            && !output.as_str().contains("private")
            && !output.as_str().contains("file:///")
            && locations.as_slice().len() == 1
    ));

    let cleared = runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-1","type":"commandExecution","status":"inProgress",
                    "aggregatedOutput":null,"commandActions":[]
                }
            }})),
            &Mapper,
        )
        .expect("explicit clear");
    let AdapterEvent::Item {
        id: cleared_id,
        payload,
        ..
    } = &cleared.events()[0]
    else {
        panic!("cleared command item")
    };
    assert_eq!(first_id, cleared_id, "clear patches the same command item");
    assert!(matches!(
        payload,
        NormalizedPayload::ToolCall { locations, .. }
            if locations.as_slice().is_empty()
    ));
}

#[test]
fn command_start_and_completion_share_one_tool_call_item() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);

    let started = runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-one","type":"commandExecution","status":"inProgress"
                }
            }})),
            &Mapper,
        )
        .expect("command start");
    let completed = runtime
        .ingest(
            &frame(json!({"method":"item/completed","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-one","type":"commandExecution","status":"completed",
                    "aggregatedOutput":"done"
                }
            }})),
            &Mapper,
        )
        .expect("command completion");

    let [
        AdapterEvent::Item {
            id: started_id,
            lifecycle: ItemLifecycle::Started,
            payload: NormalizedPayload::ToolCall { .. },
            ..
        },
    ] = started.events()
    else {
        panic!("typed command start")
    };
    let [
        AdapterEvent::Item {
            id: completed_id,
            lifecycle: ItemLifecycle::Completed,
            payload: NormalizedPayload::ToolCall { .. },
            ..
        },
    ] = completed.events()
    else {
        panic!("typed command completion")
    };
    assert_eq!(started_id, completed_id);
}

#[test]
fn a_started_command_carries_its_redacted_command_line_within_a_bounded_label() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);

    let command_line = format!(
        "{}--token sk-live-secret",
        "cargo test --package sage-long-package-name ".repeat(20)
    );
    let started = runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-label","type":"commandExecution","status":"inProgress",
                    "command":command_line
                }
            }})),
            &Mapper,
        )
        .expect("labelled command");
    let AdapterEvent::Item {
        payload:
            NormalizedPayload::ToolCall {
                label,
                input_summary,
                ..
            },
        ..
    } = &started.events()[0]
    else {
        panic!("started command call")
    };
    assert!(label.as_str().chars().count() <= 201);
    assert!(label.as_str().ends_with('…'));
    assert!(label.as_str().starts_with("cargo test --package"));
    let input = input_summary.as_ref().expect("retained command line");
    assert!(input.as_str().len() > label.as_str().len());
    assert!(!input.as_str().contains("sk-live-secret"));
    assert!(input.as_str().contains("[redacted]"));
}

#[test]
fn command_snapshots_reject_foreign_or_retired_turns_without_borrowing_prior_content() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-1","type":"commandExecution","status":"inProgress",
                    "aggregatedOutput":"turn one only"
                }
            }})),
            &Mapper,
        )
        .expect("first command");
    assert!(matches!(
        runtime.ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"foreign-thread","turnId":"turn-1","item":{
                    "id":"command-1","type":"commandExecution","status":"inProgress"
                }
            }})),
            &Mapper,
        ),
        Err(AdapterRuntimeError::Operation)
    ));
    runtime
        .ingest(
            &frame(json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}
            }})),
            &Mapper,
        )
        .expect("first terminal");
    let old = runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-1","type":"commandExecution","status":"inProgress"
                }
            }})),
            &Mapper,
        )
        .expect("old turn is ignored");
    assert!(matches!(old.events(), [AdapterEvent::IgnoredLate]));

    let next = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Start a separate turn").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-2").expect("client"),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .expect("next turn");
    let next_id = request_id(&next);
    runtime
        .ingest(
            &frame(json!({"id":next_id,"result":{"turn":{"id":"turn-2"}}})),
            &Mapper,
        )
        .expect("next turn response");
    let next_item = runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-2","item":{
                    "id":"command-1","type":"commandExecution","status":"inProgress"
                }
            }})),
            &Mapper,
        )
        .expect("new-turn command");
    assert!(matches!(
        next_item.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { locations, .. },
            ..
        }] if locations.as_slice().is_empty()
    ));
}

#[test]
fn command_output_deltas_use_one_safe_scoped_snapshot_and_flush_only_on_terminal() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);

    let started = runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-delta","type":"commandExecution","status":"inProgress",
                    "commandActions":[{
                        "type":"read","command":"type src\\lib.rs","name":"read source",
                        "path":"C:\\repo\\src\\lib.rs"
                    }]
                }
            }})),
            &Mapper,
        )
        .expect("tracked empty command");
    let AdapterEvent::Item {
        id: command_id,
        payload: NormalizedPayload::ToolCall { locations, .. },
        ..
    } = &started.events()[0]
    else {
        panic!("typed command start")
    };
    assert_eq!(locations.as_slice().len(), 1);

    let prefix = runtime
        .ingest(
            &frame(json!({"method":"item/commandExecution/outputDelta","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"command-delta","delta":"Bearer"
            }})),
            &Mapper,
        )
        .expect("bearer prefix");
    assert!(matches!(
        prefix.events(),
        [AdapterEvent::Item {
            id,
            payload: NormalizedPayload::ToolCall { output_summary: None, locations, .. },
            ..
        }] if id == command_id && locations.as_slice().len() == 1
    ));

    let credential = runtime
        .ingest(
            &frame(json!({"method":"item/commandExecution/outputDelta","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"command-delta","delta":" opaque-credential "
            }})),
            &Mapper,
        )
        .expect("bearer completion");
    let credential_debug = format!("{:?}", credential.events());
    assert!(!credential_debug.contains("opaque-credential"));
    assert!(credential_debug.contains("[redacted]"));

    runtime
        .ingest(
            &frame(json!({"method":"item/commandExecution/outputDelta","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"command-delta","delta":"C:\\Users"
            }})),
            &Mapper,
        )
        .expect("path prefix");
    let path = runtime
        .ingest(
            &frame(json!({"method":"item/commandExecution/outputDelta","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"command-delta","delta":"\\private "
            }})),
            &Mapper,
        )
        .expect("path completion");
    let path_debug = format!("{:?}", path.events());
    assert!(!path_debug.contains("Users"));
    assert!(!path_debug.contains("private"));
    assert!(path_debug.contains("[path]"));

    let terminal = runtime
        .ingest(
            &frame(json!({"method":"item/completed","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-delta","type":"commandExecution","status":"completed"
                }
            }})),
            &Mapper,
        )
        .expect("omitted terminal aggregate flushes preserved safe output");
    assert!(matches!(
        terminal.events(),
        [AdapterEvent::Item { id, lifecycle: crate::acp::lifecycle::ItemLifecycle::Completed, .. }]
            if id == command_id
    ));
    let late = runtime
        .ingest(
            &frame(
                json!({"method":"item/commandExecution/outputDelta","params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":"command-delta","delta":"late "
                }}),
            ),
            &Mapper,
        )
        .expect("late output is ignored");
    assert!(matches!(late.events(), [AdapterEvent::IgnoredLate]));
    assert!(matches!(
        runtime.ingest(
            &frame(json!({"method":"item/commandExecution/outputDelta","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"unknown-command","delta":"never "
            }})),
            &Mapper,
        ),
        Err(AdapterRuntimeError::Operation)
    ));
    assert!(matches!(
        runtime.ingest(
            &frame(json!({"method":"item/commandExecution/outputDelta","params":{
                "threadId":"foreign-thread","turnId":"turn-1","itemId":"unknown-command","delta":"never "
            }})),
            &Mapper,
        ),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn streaming_deltas_emit_complete_snapshots_and_plus_one_poison_clears_buffers() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let cases = [
        ("item/agentMessage/delta", "text-1", "hel", "lo", "hello"),
        (
            "item/reasoning/summaryTextDelta",
            "thought-1",
            "thin",
            "k",
            "think",
        ),
        ("item/plan/delta", "plan-1", "step ", "one", "step one"),
    ];
    for (method, item, first, second, expected) in cases {
        runtime
            .ingest(
                &frame(json!({"method":method,"params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":item,"delta":first
                }})),
                &Mapper,
            )
            .expect("first delta");
        let batch = runtime
            .ingest(
                &frame(json!({"method":method,"params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":item,"delta":second
                }})),
                &Mapper,
            )
            .expect("second delta");
        let AdapterEvent::Item { payload, .. } = &batch.events()[0] else {
            panic!("snapshot item")
        };
        let actual = match payload {
            NormalizedPayload::Text { text } => text.as_str(),
            NormalizedPayload::ThoughtSummary { summary } | NormalizedPayload::Plan { summary } => {
                summary.as_str()
            }
            _ => panic!("textual snapshot"),
        };
        assert_eq!(actual, expected);
    }
    for (item, first, second, forbidden) in [
        ("split-bearer", "Bearer ", "secret-value", "secret-value"),
        ("split-path", "C:", r"\Users\private", "Users"),
        ("split-key", "sk-", "private", "private"),
    ] {
        runtime
            .ingest(
                &frame(json!({"method":"item/agentMessage/delta","params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":item,"delta":first
                }})),
                &Mapper,
            )
            .expect("split prefix");
        let batch = runtime
            .ingest(
                &frame(json!({"method":"item/agentMessage/delta","params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":item,"delta":second
                }})),
                &Mapper,
            )
            .expect("split suffix");
        assert!(!format!("{:?}", batch.events()).contains(forbidden));
    }
    assert_eq!(runtime.snapshots.len(), 6);
    runtime
        .ingest(
            &frame(json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}
            }})),
            &Mapper,
        )
        .expect("terminal");
    assert_eq!(runtime.snapshots.len(), 0);

    let mut bounded = fixture_runtime();
    ready(&mut bounded);
    running_turn(&mut bounded);
    bounded
        .ingest(
            &frame(json!({"method":"item/agentMessage/delta","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"full",
                "delta":"x".repeat(262_144)
            }})),
            &Mapper,
        )
        .expect("exact bound");
    assert_eq!(bounded.snapshots.bytes(), 262_144);
    assert!(matches!(
        bounded.ingest(
            &frame(json!({"method":"item/agentMessage/delta","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"full","delta":"x"
            }})),
            &Mapper,
        ),
        Err(AdapterRuntimeError::BatchBytes)
    ));
    assert_eq!(bounded.snapshots.bytes(), 0);
    assert!(matches!(
        bounded.command(AdapterCommand::Cancel),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn model_resume_load_close_commands_are_capability_driven_and_delimiter_free() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let external = ExternalId::new("external", "thread-1").expect("external");
    let commands = [
        AdapterCommand::ModelPage { cursor: None },
        AdapterCommand::ResumeSession {
            external_session_id: external.clone(),
            native_root: r"C:\repo".into(),
            model: None,
        },
        AdapterCommand::LoadSession {
            external_session_id: external,
        },
    ];
    for command in commands {
        let egress = runtime.command(command).expect("supported command");
        assert!(!egress.frame().payload().contains(&b'\n'));
        assert!(!egress.frame().payload().contains(&b'\r'));
    }

    let mut ready_runtime = fixture_runtime();
    ready(&mut ready_runtime);
    let close = ready_runtime
        .command(AdapterCommand::CloseSession)
        .expect("close");
    assert_eq!(
        serde_json::from_slice::<Value>(close.frame().payload()).expect("close json")["method"],
        "thread/unsubscribe"
    );
}

#[test]
fn permission_request_waits_for_a_later_deny_and_never_emits_ingress_egress() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let request = frame(json!({
        "id":"approval-1","method":"item/commandExecution/requestApproval","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":"tool-1",
            "command":"secret mutation","cwd":"C:\\Users\\private"
        }
    }));
    let batch = runtime.ingest(&request, &Mapper).expect("permission");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::PermissionRequest { .. }]
    ));
    let [
        AdapterEvent::PermissionRequest {
            item_id,
            external_request_id,
            payload: NormalizedPayload::PermissionRequest { summary, .. },
            ..
        },
    ] = batch.events()
    else {
        panic!("permission request");
    };
    assert_eq!(item_id.external_id.as_str(), "tool-1");
    assert_eq!(external_request_id.as_str(), "string:approval-1");
    assert_eq!(summary.as_str(), "Run a command");
    assert!(batch.egress().is_empty());
    assert!(batch.actual_bytes() <= 262_144);

    let egress = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: crate::acp::contract::SageServerRequestId::parse("string:approval-1")
                .unwrap(),
            decision: PermissionDecision::Deny,
        })
        .expect("later deny");
    assert!(egress.token().is_mandatory());
    assert_eq!(
        serde_json::from_slice::<Value>(egress.frame().payload()).expect("deny json")["result"]["decision"],
        "decline"
    );
    assert!(batch.actual_bytes() <= 262_144);
    let token = egress.token().clone();
    runtime
        .commit_mandatory_egress(&token)
        .expect("durable deny");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("deny queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("deny written");

    let duplicate = runtime.ingest(&request, &Mapper).expect("duplicate");
    assert!(matches!(
        duplicate.events(),
        [AdapterEvent::IgnoredDuplicate]
    ));
    assert_eq!(duplicate.egress().len(), 1);
    assert!(duplicate.egress()[0].token().is_mandatory());

    let mut rejected = fixture_runtime();
    ready(&mut rejected);
    running_turn(&mut rejected);
    let _batch = rejected.ingest(&request, &Mapper).expect("permission");
    let egress = rejected
        .command(AdapterCommand::ResolvePermission {
            request_id: crate::acp::contract::SageServerRequestId::parse("string:approval-1")
                .unwrap(),
            decision: PermissionDecision::Deny,
        })
        .expect("later deny");
    assert_eq!(
        rejected.resolve_egress(egress.token().clone(), EgressResolution::QueueRejected),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
    );
}

#[test]
fn allow_once_on_codex_does_not_emit_denial_and_records_resolved_allowed() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let request = frame(json!({
        "id":"approval-allow","method":"item/commandExecution/requestApproval","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":"tool-allow"
        }
    }));
    let first = runtime.ingest(&request, &Mapper).expect("permission");
    assert!(first.egress().is_empty());

    let allow = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: crate::acp::contract::SageServerRequestId::parse("string:approval-allow")
                .unwrap(),
            decision: PermissionDecision::AllowOnce,
        })
        .expect("later allow");
    assert!(!allow.token().is_mandatory());
    runtime
        .resolve_egress(allow.token().clone(), EgressResolution::Queued)
        .expect("allow queued");
    let queued_duplicate = runtime.ingest(&request, &Mapper).expect("queued duplicate");
    assert!(queued_duplicate.egress().is_empty());

    runtime
        .resolve_egress(allow.into_token(), EgressResolution::Written)
        .expect("allow written");
    let resolved_duplicate = runtime
        .ingest(&request, &Mapper)
        .expect("resolved duplicate");
    assert!(resolved_duplicate.egress().is_empty());
}

#[test]
fn codex_unknown_command_decisions_offer_reject_only() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "id":"unknown-command","method":"item/commandExecution/requestApproval","params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":"unknown-tool",
                    "availableDecisions":["futureDecision",42]
                }
            })),
            &Mapper,
        )
        .expect("unknown decisions permission");
    let AdapterEvent::PermissionRequest {
        payload:
            NormalizedPayload::PermissionRequest {
                offered_option_kinds: Some(kinds),
                ..
            },
        ..
    } = &batch.events()[0]
    else {
        panic!("permission options")
    };
    assert_eq!(
        kinds.as_slice(),
        &[crate::acp::intent::PermissionOptionKind::RejectOnce]
    );
}

#[test]
fn codex_always_allow_uses_session_decision_only_for_supported_requests() {
    let mut limited = fixture_runtime();
    ready(&mut limited);
    running_turn(&mut limited);
    let batch = limited
        .ingest(
            &frame(json!({
                "id":"limited-command","method":"item/commandExecution/requestApproval","params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":"limited-tool",
                    "availableDecisions":["decline","accept"]
                }
            })),
            &Mapper,
        )
        .expect("limited permission");
    let AdapterEvent::PermissionRequest {
        payload:
            NormalizedPayload::PermissionRequest {
                offered_option_kinds: Some(kinds),
                ..
            },
        ..
    } = &batch.events()[0]
    else {
        panic!("permission options")
    };
    assert_eq!(
        kinds.as_slice(),
        &[
            crate::acp::intent::PermissionOptionKind::RejectOnce,
            crate::acp::intent::PermissionOptionKind::AllowOnce,
        ]
    );
    assert!(
        limited
            .command(AdapterCommand::ResolvePermission {
                request_id: crate::acp::contract::SageServerRequestId::parse(
                    "string:limited-command"
                )
                .unwrap(),
                decision: PermissionDecision::AllowAlways,
            })
            .is_err()
    );

    let malformed = limited
        .ingest(
            &frame(json!({
                "id":"malformed-command","method":"item/commandExecution/requestApproval","params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":"malformed-tool",
                    "availableDecisions":"acceptForSession"
                }
            })),
            &Mapper,
        )
        .expect("malformed decisions permission");
    let AdapterEvent::PermissionRequest {
        payload:
            NormalizedPayload::PermissionRequest {
                offered_option_kinds: Some(kinds),
                ..
            },
        ..
    } = &malformed.events()[0]
    else {
        panic!("permission options")
    };
    assert_eq!(
        kinds.as_slice(),
        &[crate::acp::intent::PermissionOptionKind::RejectOnce]
    );
    assert!(
        limited
            .command(AdapterCommand::ResolvePermission {
                request_id: crate::acp::contract::SageServerRequestId::parse(
                    "string:malformed-command"
                )
                .unwrap(),
                decision: PermissionDecision::AllowAlways,
            })
            .is_err()
    );

    let ordered = limited
        .ingest(
            &frame(json!({
                "id":"ordered-command","method":"item/commandExecution/requestApproval","params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":"ordered-tool",
                    "availableDecisions":["decline","acceptForSession","accept"]
                }
            })),
            &Mapper,
        )
        .expect("ordered permission");
    let AdapterEvent::PermissionRequest {
        payload:
            NormalizedPayload::PermissionRequest {
                offered_option_kinds: Some(kinds),
                ..
            },
        ..
    } = &ordered.events()[0]
    else {
        panic!("permission options")
    };
    assert_eq!(
        kinds.as_slice(),
        &[
            crate::acp::intent::PermissionOptionKind::RejectOnce,
            crate::acp::intent::PermissionOptionKind::AllowAlways,
            crate::acp::intent::PermissionOptionKind::AllowOnce,
        ]
    );

    for (method, null_decisions) in [
        ("item/commandExecution/requestApproval", false),
        ("item/commandExecution/requestApproval", true),
        ("item/fileChange/requestApproval", false),
    ] {
        let mut runtime = fixture_runtime();
        ready(&mut runtime);
        running_turn(&mut runtime);
        let mut request = json!({
            "id":"approval-session","method":method,"params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"tool-session"
            }
        });
        if null_decisions {
            request["params"]["availableDecisions"] = serde_json::Value::Null;
        }
        let request = frame(request);
        let batch = runtime.ingest(&request, &Mapper).expect("permission");
        let AdapterEvent::PermissionRequest {
            payload:
                NormalizedPayload::PermissionRequest {
                    offered_option_kinds: Some(kinds),
                    ..
                },
            ..
        } = &batch.events()[0]
        else {
            panic!("permission options")
        };
        assert!(
            kinds
                .as_slice()
                .contains(&crate::acp::intent::PermissionOptionKind::AllowAlways)
        );
        let reply = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: crate::acp::contract::SageServerRequestId::parse(
                    "string:approval-session",
                )
                .unwrap(),
                decision: PermissionDecision::AllowAlways,
            })
            .expect("session approval");
        let response: serde_json::Value = serde_json::from_slice(reply.frame().payload()).unwrap();
        assert_eq!(response["result"]["decision"], "acceptForSession");
    }

    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "id":"approval-profile","method":"item/permissions/requestApproval","params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":"tool-profile"
                }
            })),
            &Mapper,
        )
        .expect("permission");
    let AdapterEvent::PermissionRequest {
        payload:
            NormalizedPayload::PermissionRequest {
                offered_option_kinds: Some(kinds),
                ..
            },
        ..
    } = &batch.events()[0]
    else {
        panic!("permission options")
    };
    assert!(
        !kinds
            .as_slice()
            .contains(&crate::acp::intent::PermissionOptionKind::AllowAlways)
    );
    assert!(
        runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: crate::acp::contract::SageServerRequestId::parse(
                    "string:approval-profile"
                )
                .unwrap(),
                decision: PermissionDecision::AllowAlways,
            })
            .is_err()
    );
}

#[test]
fn durable_permission_id_correlates_to_codex_jsonrpc_request_id() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "id":"approval-correlation","method":"item/commandExecution/requestApproval","params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":"tool-correlation"
                }
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
    assert_eq!(external_request_id.as_str(), "string:approval-correlation");
}

#[test]
fn allow_once_preserves_numeric_codex_jsonrpc_request_id() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    runtime
        .ingest(
            &frame(json!({
                "id":17,"method":"item/commandExecution/requestApproval","params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":"tool-numeric"
                }
            })),
            &Mapper,
        )
        .expect("numeric permission");
    let allow = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: crate::acp::contract::SageServerRequestId::parse("number:17").unwrap(),
            decision: PermissionDecision::AllowOnce,
        })
        .expect("numeric allow");
    let body: Value = serde_json::from_slice(allow.frame().payload()).expect("json");
    assert_eq!(body["id"], json!(17));
    assert!(body["id"].is_number());
}

#[test]
fn numeric_and_string_codex_jsonrpc_request_ids_do_not_alias() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let numeric = frame(json!({
        "id":17,"method":"item/commandExecution/requestApproval","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":"tool-numeric"
        }
    }));
    let string = frame(json!({
        "id":"17","method":"item/commandExecution/requestApproval","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":"tool-string"
        }
    }));
    let numeric_batch = runtime
        .ingest(&numeric, &Mapper)
        .expect("numeric permission");
    let string_batch = runtime.ingest(&string, &Mapper).expect("string permission");
    let AdapterEvent::PermissionRequest {
        external_request_id: numeric_id,
        ..
    } = &numeric_batch.events()[0]
    else {
        panic!("numeric permission request")
    };
    let AdapterEvent::PermissionRequest {
        external_request_id: string_id,
        ..
    } = &string_batch.events()[0]
    else {
        panic!("string permission request")
    };
    assert_eq!(numeric_id.as_str(), "number:17");
    assert_eq!(string_id.as_str(), "string:17");
    assert_ne!(numeric_id, string_id);

    let string_allow = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: crate::acp::contract::SageServerRequestId::parse("string:17").unwrap(),
            decision: PermissionDecision::AllowOnce,
        })
        .expect("string allow");
    assert_eq!(
        serde_json::from_slice::<Value>(string_allow.frame().payload()).expect("string json")["id"],
        json!("17")
    );
    let numeric_allow = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: crate::acp::contract::SageServerRequestId::parse("number:17").unwrap(),
            decision: PermissionDecision::AllowOnce,
        })
        .expect("numeric remains pending");
    assert_eq!(
        serde_json::from_slice::<Value>(numeric_allow.frame().payload()).expect("numeric json")["id"],
        json!(17)
    );
}

#[test]
fn permissions_approval_allow_once_uses_the_permissions_response_shape() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let request = frame(json!({
        "id":"approval-permissions","method":"item/permissions/requestApproval","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":"tool-permissions"
        }
    }));
    runtime.ingest(&request, &Mapper).expect("permission");

    let allow = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: crate::acp::contract::SageServerRequestId::parse(
                "string:approval-permissions",
            )
            .unwrap(),
            decision: PermissionDecision::AllowOnce,
        })
        .expect("later allow");
    assert_eq!(
        serde_json::from_slice::<Value>(allow.frame().payload()).expect("allow json")["result"],
        json!({"permissions": {}})
    );
}

#[test]
fn malformed_unknown_capacity_plus_one_and_egress_rollback_are_deterministic() {
    let mut runtime = fixture_runtime();
    assert!(matches!(
        runtime.ingest(&frame(json!("not-an-object")), &Mapper),
        Err(AdapterRuntimeError::Frame)
    ));
    let unknown = runtime
        .ingest(
            &frame(json!({"method":"future/event","params":{}})),
            &Mapper,
        )
        .expect("unknown event");
    assert!(matches!(unknown.events(), [AdapterEvent::IgnoredLate]));

    initialize(&mut runtime);
    for _ in 0..16 {
        runtime
            .command(AdapterCommand::ModelPage { cursor: None })
            .expect("bounded request");
    }
    assert!(matches!(
        runtime.command(AdapterCommand::ModelPage { cursor: None }),
        Err(AdapterRuntimeError::Operation)
    ));

    let mut rollback = fixture_runtime();
    ready(&mut rollback);
    let first = rollback
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "first").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-first").expect("client"),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .expect("first prompt");
    rollback
        .resolve_egress(first.into_token(), EgressResolution::QueueRejected)
        .expect("rollback");
    assert!(
        rollback
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "second").expect("prompt"),
                client_message_id: RequiredText::new("client", "client-second").expect("client"),
                access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .is_ok()
    );

    let mut zero_write = fixture_runtime();
    ready(&mut zero_write);
    let first = zero_write
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "first").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-zero").expect("client"),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .expect("first prompt");
    let token = first.token().clone();
    zero_write
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queue admitted");
    zero_write
        .resolve_egress(token, EgressResolution::WriteZeroBytes)
        .expect("definite zero-byte write rollback");
    assert!(
        zero_write
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "retry").expect("prompt"),
                client_message_id: RequiredText::new("client", "client-retry").expect("client"),
                access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .is_ok()
    );

    let mut uncertain = fixture_runtime();
    ready(&mut uncertain);
    let first = uncertain
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "possibly written").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-unknown").expect("client"),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .expect("first prompt");
    uncertain
        .resolve_egress(first.into_token(), EgressResolution::DeliveryUnknown)
        .expect("unknown delivery stays reserved");
    assert!(matches!(
        uncertain.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "must not replay").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-no-replay").expect("client"),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        }),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn harness_neutral_batches_enforce_count_and_actual_byte_limits() {
    assert!(AdapterBatch::new(vec![AdapterEvent::Acknowledged; 128], Vec::new()).is_ok());
    assert!(matches!(
        AdapterBatch::new(vec![AdapterEvent::Acknowledged; 129], Vec::new()),
        Err(AdapterRuntimeError::BatchCount)
    ));

    let large = || {
        AdapterEvent::Unsupported(NormalizedPayload::Unsupported {
            external_kind: SafeText::new("kind", "future").expect("kind"),
            summary: SafeText::new("summary", "x".repeat(16_384)).expect("summary"),
        })
    };
    let batch = AdapterBatch::new((0..128).map(|_| large()).collect(), Vec::new())
        .expect("two MiB remains below four MiB journal batch");
    assert!(batch.actual_bytes() > 2_000_000);
    let (events, egress) = batch.into_parts();
    assert_eq!(events.len(), 128);
    assert!(egress.is_empty());

    let identity = AdapterIdentity {
        binding_digest: [1; 32],
        runtime_epoch: RuntimeEpoch::new(1).expect("epoch"),
        session_id: SageAgentSessionId::parse("session-batch").expect("session"),
        external_id: ExternalId::new("item", "item-large").expect("item"),
        turn_id: Some(ExternalId::new("turn", "turn-large").expect("turn")),
        turn_client_message_id: Some(RequiredText::new("client", "client-large").expect("client")),
    };
    let item = || AdapterEvent::Item {
        id: identity.clone(),
        lifecycle: crate::acp::lifecycle::ItemLifecycle::Streaming,
        tool_state: None,
        payload: NormalizedPayload::Text {
            text: SafeText::new("text", "x".repeat(262_144)).expect("text"),
        },
    };
    assert!(AdapterBatch::new((0..15).map(|_| item()).collect(), Vec::new()).is_ok());
    assert!(matches!(
        AdapterBatch::new((0..16).map(|_| item()).collect(), Vec::new()),
        Err(AdapterRuntimeError::BatchBytes)
    ));
}

mod cancel_contract_tests;

#[path = "tests/model_page_rejection.rs"]
mod model_page_rejection;

#[test]
fn slash_commands_are_empty_when_codex_has_not_advertised_any() {
    let runtime = CodexRuntime::fixture(Platform::WindowsX86_64);
    assert!(runtime.available_commands().snapshot().commands.is_empty());
    assert!(
        runtime
            .available_commands()
            .validate(0, "/invented")
            .is_err()
    );
}
