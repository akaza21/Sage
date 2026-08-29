use serde_json::{Value, json};

use super::*;
use crate::acp::{
    bindings::{
        adapter::{
            AdapterCommand, AdapterEvent, AdapterIdentity, AdapterRuntimeError, EgressResolution,
            HarnessAdapter, ResourceMapper,
        },
        codex::{
            CodexProjectRootAuthority, Platform, parse_packet_metadata, parse_security_policy,
        },
    },
    contract::{
        ProjectGeneration, RequiredText, RuntimeEpoch, SafeText, SageAgentSessionId, SageProjectId,
        SageRootId,
    },
    intent::{ExternalId, NormalizedPayload, Prompt, SafeResourceRef},
    runtime::permissions::PermissionDecision,
    transport::{Frame, IncrementalFrameDecoder},
};
use crate::project_registry::ProjectRootAuthority;
use crate::{error::AppError, persistence::Database, state::AppState};

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

#[test]
fn thread_started_notification_completes_the_pending_new_session() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let _request = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .expect("new session");

    let batch = runtime
        .ingest(
            &frame(json!({"method":"thread/started","params":{
                "thread":{"id":"thread-0146"}
            }})),
            &Mapper,
        )
        .expect("Codex 0.146 thread-started notification");

    assert!(matches!(
        batch.events(),
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
            &frame(json!({"id":id,"result":{"thread":{"id":"thread-0146"}}})),
            &Mapper,
        )
        .expect("thread-start response");

    let late = runtime
        .ingest(
            &frame(json!({"method":"thread/started","params":{
                "thread":{"id":"thread-0146"}
            }})),
            &Mapper,
        )
        .expect("late Codex 0.146 thread-started notification");

    assert!(matches!(late.events(), [AdapterEvent::IgnoredLate]));
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
fn notification_first_turn_start_ignores_the_late_response_state_regression() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: RequiredText::new("client", "notification-first").unwrap(),
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
fn prompt_and_interrupt_rpc_errors_emit_terminal_failure_without_replay() {
    let mut prompt_failed = fixture_runtime();
    ready(&mut prompt_failed);
    let prompt = prompt_failed
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Never replay").unwrap(),
            client_message_id: RequiredText::new("client", "failed-prompt").unwrap(),
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

    let path = std::env::temp_dir().join(format!(
        "sage-observed-codex-runtime-{}.cmd",
        uuid::Uuid::new_v4()
    ));
    fs::write(&path, "@echo off\r\n").unwrap();
    let binding = crate::acp::bindings::codex::AdmittedCodexBinding::admit_observed(
        path.clone(),
        "codex 0.146.0".into(),
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
            "userAgent": "sage/0.146.0 (Windows 10.0.26200; x86_64) terminal (sage; 0.1.0)",
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
    fs::remove_file(path).unwrap();
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
    assert_eq!(
        summary.as_str(),
        "Codex requested additional authority; awaiting your decision"
    );
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
