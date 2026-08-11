use serde_json::{Value, json};

use super::*;
use crate::acp::{
    bindings::{
        adapter::{
            AdapterCommand, AdapterEvent, AdapterIdentity, AdapterRuntimeError, EgressResolution,
            HarnessAdapter,
        },
        codex::{Platform, parse_packet_metadata, parse_security_policy},
    },
    contract::{ProjectGeneration, RequiredText, SafeText, SageProjectId, SageRootId},
    intent::{ExternalId, NormalizedPayload, Prompt, SafeResourceRef},
    transport::IncrementalFrameDecoder,
};
use crate::project_registry::ProjectRootAuthority;

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
) -> ProjectRootAuthority {
    ProjectRootAuthority::for_test(
        project_id,
        generation,
        root_id,
        &std::env::current_dir().unwrap(),
    )
    .unwrap()
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
                "userAgent":"sage/0.145.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
                "codexHome":"C:\\SageHarnesses\\CodexState\\codex-app-server-0.145.0-7b0ab167",
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
            &frame(json!({"id":id,"result":{"thread":{"id":"thread-1"}}})),
            &Mapper,
        )
        .expect("session response");
}

fn running_turn(runtime: &mut CodexRuntime) {
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-1").expect("client"),
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
            &launch_root(
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
fn runtime_retains_admission_authority_and_frozen_environment_until_teardown() {
    let registration = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let binding = registration.refresh().unwrap();
    let expected_environment = binding.environment().entries().to_vec();
    let retained = std::sync::Arc::downgrade(&binding);
    let runtime = CodexRuntime::from_admitted_binding(
        binding,
        &launch_root(
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
    let project_root = launch_root(
        launch_project_id(),
        ProjectGeneration::ZERO,
        launch_root_id(),
    );
    let working_directory = project_root.as_path().to_owned();
    let runtime = CodexRuntime::from_admitted_binding(
        binding,
        &project_root,
        RuntimeEpoch::new(23).expect("epoch"),
        SageAgentSessionId::parse("session-launch-contract").expect("session"),
    )
    .unwrap();

    let spec = runtime
        .launch_spec(project_root)
        .expect("admitted launch contract");

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
        ["CODEX_HOME", "SystemRoot", "TEMP", "TMP", "WINDIR"]
    );
    for inherited in ["HOME", "USERPROFILE", "PATH", "OPENAI_API_KEY"] {
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
    let runtime = fixture_runtime();
    let project_root = launch_root(
        launch_project_id(),
        ProjectGeneration::ZERO,
        launch_root_id(),
    );

    assert_eq!(
        runtime.launch_spec(project_root).err(),
        Some(AdapterRuntimeError::LaunchUnavailable)
    );
}

#[cfg(windows)]
#[test]
fn launch_contract_rejects_foreign_root_context_and_admission_drift() {
    let registration = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let binding = registration.refresh().unwrap();
    let runtime = CodexRuntime::from_admitted_binding(
        binding,
        &launch_root(
            launch_project_id(),
            ProjectGeneration::ZERO,
            launch_root_id(),
        ),
        RuntimeEpoch::new(24).expect("epoch"),
        SageAgentSessionId::parse("session-launch-drift").expect("session"),
    )
    .unwrap();
    for foreign in [
        launch_root(
            SageProjectId::parse("project-foreign").unwrap(),
            ProjectGeneration::ZERO,
            launch_root_id(),
        ),
        launch_root(
            launch_project_id(),
            ProjectGeneration::new(1).unwrap(),
            launch_root_id(),
        ),
        launch_root(
            launch_project_id(),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-foreign").unwrap(),
        ),
    ] {
        assert_eq!(
            runtime.launch_spec(foreign).err(),
            Some(AdapterRuntimeError::LaunchUnavailable)
        );
    }

    registration.induce_drift_for_test();
    assert_eq!(
        runtime
            .launch_spec(launch_root(
                launch_project_id(),
                ProjectGeneration::ZERO,
                launch_root_id(),
            ))
            .err(),
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
            payload: NormalizedPayload::ToolUpdateResult { .. },
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
    assert!(matches!(usage.events(), [AdapterEvent::Usage { .. }]));

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
    assert!(matches!(
        terminal.events(),
        [AdapterEvent::Turn {
            state: crate::acp::lifecycle::TurnState::Interrupted,
            ..
        }]
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
fn permission_denial_is_mandatory_bounded_and_fail_closed() {
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
    assert_eq!(batch.egress().len(), 1);
    let egress = &batch.egress()[0];
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
    assert!(duplicate.egress()[0].token().is_mandatory());

    let mut rejected = fixture_runtime();
    ready(&mut rejected);
    running_turn(&mut rejected);
    let batch = rejected.ingest(&request, &Mapper).expect("permission");
    assert_eq!(
        rejected.resolve_egress(
            batch.egress()[0].token().clone(),
            EgressResolution::QueueRejected
        ),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
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
    assert!(matches!(unknown.events(), [AdapterEvent::Unsupported(_)]));

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
            })
            .is_ok()
    );

    let mut zero_write = fixture_runtime();
    ready(&mut zero_write);
    let first = zero_write
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "first").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-zero").expect("client"),
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
            })
            .is_ok()
    );

    let mut uncertain = fixture_runtime();
    ready(&mut uncertain);
    let first = uncertain
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "possibly written").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-unknown").expect("client"),
        })
        .expect("first prompt");
    uncertain
        .resolve_egress(first.into_token(), EgressResolution::DeliveryUnknown)
        .expect("unknown delivery stays reserved");
    assert!(matches!(
        uncertain.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "must not replay").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-no-replay").expect("client"),
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
