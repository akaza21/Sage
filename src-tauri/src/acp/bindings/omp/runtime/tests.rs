use serde_json::{Value, json};

use super::*;
use crate::acp::{
    bindings::{
        adapter::{
            AdapterCommand, AdapterEvent, AdapterRuntimeError, EgressResolution, HarnessAdapter,
        },
        omp::{
            ArtifactIdentity, CandidateManifest, CuratedOmpBinding, OMP_BUN_MINIMUM, OMP_LICENSE,
            OMP_PACKAGE, OMP_SOURCE_COMMIT, OMP_VERSION, OmpPlatform,
        },
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SageRootId, SageServerRequestId},
    intent::{ExternalId, NormalizedPayload, Prompt, SafeResourceRef},
    transport::IncrementalFrameDecoder,
};

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        path: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        (path == r"C:\fixture\src\lib.rs")
            .then(|| {
                SafeResourceRef::new(
                    SageRootId::parse("root-omp-fixture").expect("root"),
                    "src/lib.rs",
                )
                .expect("resource")
            })
            .ok_or(crate::acp::intent::UnavailableResourceReason::Foreign)
    }
}

fn artifact(path: &str, byte: char) -> ArtifactIdentity {
    ArtifactIdentity {
        relative_path: path.into(),
        sha256: byte.to_string().repeat(64),
        bytes: 42,
    }
}

fn research_binding() -> ResearchBinding {
    let entrypoint = artifact("omp/bin/omp", 'a');
    let runtime = artifact("bun/bun.exe", 'b');
    let native_addons = vec![artifact("omp/addons/parser.node", 'c')];
    let curated = CuratedOmpBinding {
        platform: OmpPlatform::WindowsX64,
        acp_protocol_version: "1".into(),
        bun_version: OMP_BUN_MINIMUM.into(),
        entrypoint: entrypoint.clone(),
        runtime: runtime.clone(),
        native_addons: native_addons.clone(),
    };
    curated
        .validate_candidate(&CandidateManifest {
            platform: OmpPlatform::WindowsX64,
            package_name: OMP_PACKAGE.into(),
            package_version: OMP_VERSION.into(),
            source_commit: OMP_SOURCE_COMMIT.into(),
            license: OMP_LICENSE.into(),
            acp_protocol_version: "1".into(),
            bun_version: OMP_BUN_MINIMUM.into(),
            entrypoint,
            runtime,
            native_addons,
            fixed_args: vec!["acp".into()],
            isolated_state: true,
            isolated_auth: true,
            ambient_extensions_disabled: true,
            ambient_configuration_disabled: true,
            dynamic_downloads_disabled: true,
            browser_subprocess_lsp_dap_disabled: true,
            offline_and_telemetry_reviewed: true,
            read_only_authority_verified: true,
            native_acp_ndjson_verified: true,
            disconnect_teardown_verified: true,
        })
        .expect("binding")
}

fn fixture_runtime(epoch: u64) -> OmpRuntime {
    let binding = research_binding();
    OmpRuntime::fixture(
        binding.capabilities(),
        RuntimeEpoch::new(epoch).expect("epoch"),
        SageAgentSessionId::parse(format!("session-omp-{epoch}")).expect("session"),
    )
}

fn raw_frame(raw: &[u8]) -> Frame {
    let mut bytes = raw.to_vec();
    bytes.push(b'\n');
    let mut decoder = IncrementalFrameDecoder::new();
    let mut frame = None;
    for chunk in bytes.chunks(8_192) {
        let next = decoder.push_chunk(chunk).expect("frame");
        assert!(frame.is_none());
        frame = next.or(frame);
    }
    frame.expect("complete")
}

fn frame(value: Value) -> Frame {
    raw_frame(&serde_json::to_vec(&value).expect("json"))
}

fn request(egress: &AdapterEgress) -> Value {
    serde_json::from_slice(egress.frame().payload()).expect("request")
}

fn deliver(runtime: &mut OmpRuntime, egress: AdapterEgress) -> Value {
    let request = request(&egress);
    let token = egress.into_token();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    request
}

fn response(request: &Value, result: Value) -> Frame {
    frame(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}

fn initialize(runtime: &mut OmpRuntime, load: bool) {
    let command = runtime
        .command(AdapterCommand::Initialize)
        .expect("initialize");
    let request = deliver(runtime, command);
    let batch = runtime
        .ingest(
            &response(
                &request,
                json!({"protocolVersion":1,"agentCapabilities":{"loadSession":load}}),
            ),
            &Mapper,
        )
        .expect("initialized");
    assert!(matches!(batch.events(), [AdapterEvent::Initialized { .. }]));
}

fn new_session(runtime: &mut OmpRuntime) {
    let command = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture".into(),
            model: None,
        })
        .expect("new session");
    let request = deliver(runtime, command);
    let batch = runtime
        .ingest(
            &response(
                &request,
                json!({
                    "sessionId":"omp-session-1",
                    "models":{"currentModelId":"model-a","availableModels":[
                        {"modelId":"model-a","name":"Model A"},
                        {"modelId":"model-b","name":"Model B"}
                    ]}
                }),
            ),
            &Mapper,
        )
        .expect("session ready");
    assert!(matches!(
        batch.events(),
        [
            AdapterEvent::Models { .. },
            AdapterEvent::SessionReady { loaded: false, .. }
        ]
    ));
}

fn running(runtime: &mut OmpRuntime) -> Value {
    let command = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect read-only").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-omp-1").expect("client"),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .expect("prompt");
    deliver(runtime, command)
}

fn permission(id: u64, tool: &str) -> Frame {
    frame(json!({
        "jsonrpc":"2.0","id":id,"method":"session/request_permission",
        "params":{"sessionId":"omp-session-1","toolCall":{
            "toolCallId":tool,"kind":"execute","title":"bash",
            "rawInput":{"command":"secret C:\\Users\\private"}
        },"options":[]}
    }))
}

#[test]
fn exact_candidate_remains_launch_unavailable() {
    let binding = research_binding();
    assert!(matches!(
        OmpRuntime::from_binding(
            &binding,
            [1; 32],
            RuntimeEpoch::new(1).expect("epoch"),
            SageAgentSessionId::parse("session-omp-launch").expect("session")
        ),
        Err(AdapterRuntimeError::LaunchUnavailable)
    ));
}

#[test]
fn negotiated_lifecycle_models_and_load_are_capability_honest() {
    let mut runtime = fixture_runtime(2);
    initialize(&mut runtime, true);
    let capabilities = runtime.capabilities().expect("capabilities");
    assert!(capabilities.new_session && capabilities.load_session);
    assert!(capabilities.prompt && capabilities.cancel);
    assert!(!capabilities.resume_session && !capabilities.close_session);
    assert!(matches!(
        runtime.command(AdapterCommand::NewSession {
            native_root: r"C:\fixture".into(),
            model: Some(RequiredText::new("model", "model-a").expect("model")),
        }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    new_session(&mut runtime);
    assert_eq!(runtime.capabilities().expect("capabilities").model_count, 2);
    assert!(matches!(
        runtime.command(AdapterCommand::NewSession {
            native_root: r"C:\fixture".into(),
            model: Some(RequiredText::new("model", "model-b").expect("model")),
        }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    let exact = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture".into(),
            model: Some(RequiredText::new("model", "model-a").expect("model")),
        })
        .expect("immutable current model");
    assert!(!String::from_utf8_lossy(exact.frame().payload()).contains("model-a"));
    let exact = deliver(&mut runtime, exact);
    runtime
        .ingest(
            &response(
                &exact,
                json!({"sessionId":"omp-session-2","models":{"currentModelId":"model-a","availableModels":[{"modelId":"model-a","name":"Model A"}]}}),
            ),
            &Mapper,
        )
        .expect("second session");
    let load = runtime
        .command(AdapterCommand::LoadSession {
            external_session_id: ExternalId::new("id", "omp-session-2").expect("id"),
        })
        .expect("load");
    assert_eq!(request(&load)["method"], "session/load");
    let load = deliver(&mut runtime, load);
    let loaded = runtime
        .ingest(
            &response(
                &load,
                json!({"sessionId":"omp-session-2","models":{"currentModelId":"model-a","availableModels":[{"modelId":"model-a","name":"Model A"}]}}),
            ),
            &Mapper,
        )
        .expect("loaded session");
    assert!(matches!(
        loaded.events(),
        [
            AdapterEvent::Models { .. },
            AdapterEvent::SessionReady { loaded: true, .. }
        ]
    ));
    assert!(matches!(
        runtime.command(AdapterCommand::ModelPage { cursor: None }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    assert!(matches!(
        runtime.command(AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("id", "omp-session-2").expect("id"),
            native_root: r"C:\fixture".into(),
            model: None,
        }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    assert!(matches!(
        runtime.command(AdapterCommand::CloseSession),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
}

#[test]
fn delivery_resolution_rolls_back_zero_and_poison_unknown_without_replay() {
    let mut rejected = fixture_runtime(3);
    let first = rejected.command(AdapterCommand::Initialize).expect("first");
    rejected
        .resolve_egress(first.into_token(), EgressResolution::QueueRejected)
        .expect("rollback");
    assert!(rejected.command(AdapterCommand::Initialize).is_ok());

    let mut zero = fixture_runtime(4);
    let first = zero.command(AdapterCommand::Initialize).expect("first");
    let token = first.into_token();
    zero.resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    zero.resolve_egress(token, EgressResolution::WriteZeroBytes)
        .expect("zero rollback");
    assert!(zero.command(AdapterCommand::Initialize).is_ok());

    let mut ordered = fixture_runtime(5);
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
        .expect("written");

    let mut unknown = fixture_runtime(6);
    let first = unknown.command(AdapterCommand::Initialize).expect("first");
    unknown
        .resolve_egress(first.into_token(), EgressResolution::DeliveryUnknown)
        .expect("unknown");
    assert!(matches!(
        unknown.command(AdapterCommand::Initialize),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn unresolved_permission_decision_reports_missing_intent() {
    let mut runtime = fixture_runtime(7);
    assert!(matches!(
        runtime.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("request-omp-missing").expect("request"),
            decision: crate::acp_protocol::permissions::PermissionDecision::Deny,
        }),
        Err(AdapterRuntimeError::PermissionIntent)
    ));
}

#[test]
fn turn_items_typed_diff_cancel_duplicate_and_late_are_normalized() {
    let mut runtime = fixture_runtime(7);
    initialize(&mut runtime, false);
    new_session(&mut runtime);
    let prompt = running(&mut runtime);
    for (index, line) in include_str!("../../../../../testdata/acp/omp/read_only_turn.jsonl")
        .lines()
        .enumerate()
    {
        let batch = runtime
            .ingest(&raw_frame(line.as_bytes()), &Mapper)
            .expect("update");
        if index == 0 {
            assert!(matches!(
                batch.events(),
                [
                    AdapterEvent::Turn { .. },
                    AdapterEvent::Item {
                        payload: NormalizedPayload::Text { .. },
                        ..
                    }
                ]
            ));
        }
        if index == 3 {
            assert_eq!(batch.egress().len(), 1);
            assert_eq!(request(&batch.egress()[0])["method"], "session/cancel");
            assert!(
                batch.egress()[0].token().is_control() && batch.egress()[0].token().is_cancel()
            );
            let token = batch.egress()[0].token().clone();
            runtime
                .resolve_egress(token.clone(), EgressResolution::Queued)
                .expect("authority cancel queued");
            runtime
                .resolve_egress(token, EgressResolution::Written)
                .expect("authority cancel written");
        }
        assert!(!format!("{:?}", batch.events()).contains(r"C:\fixture"));
    }
    let terminal_frame = response(&prompt, json!({"stopReason":"cancelled"}));
    let terminal = runtime.ingest(&terminal_frame, &Mapper).expect("terminal");
    assert!(matches!(terminal.events(), [AdapterEvent::Turn { .. }]));
    assert!(matches!(
        runtime
            .ingest(&terminal_frame, &Mapper)
            .expect("duplicate")
            .events(),
        [AdapterEvent::IgnoredDuplicate]
    ));
    let late = frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
        "sessionId":"omp-session-1","update":{"sessionUpdate":"agent_message_chunk",
        "content":{"type":"text","text":"late"}}
    }}));
    assert!(matches!(
        runtime.ingest(&late, &Mapper).expect("late").events(),
        [AdapterEvent::IgnoredLate]
    ));
}

#[test]
fn mandatory_denial_is_correlated_replay_bounded_and_fail_closed() {
    let mut runtime = fixture_runtime(8);
    initialize(&mut runtime, false);
    new_session(&mut runtime);
    running(&mut runtime);
    let batch = runtime
        .ingest(&permission(17, "permission-tool"), &Mapper)
        .expect("deny");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::PermissionRequest { .. }]
    ));
    assert_eq!(batch.egress().len(), 1);
    assert!(!String::from_utf8_lossy(batch.egress()[0].frame().payload()).contains("Users"));
    let token = batch.egress()[0].token().clone();
    assert_eq!(
        runtime.resolve_egress(token.clone(), EgressResolution::Queued),
        Err(AdapterRuntimeError::EgressIntent)
    );
    runtime.commit_mandatory_egress(&token).expect("commit");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    let replay = runtime
        .ingest(&permission(17, "permission-tool"), &Mapper)
        .expect("replay");
    assert_eq!(replay.egress().len(), 1);
    let token = replay.egress()[0].token().clone();
    assert_eq!(token.deny_request(), Some(("17", true)));
    runtime
        .commit_mandatory_egress(&token)
        .expect("replay commit");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("replay queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("replay written");

    let mut failed = fixture_runtime(9);
    initialize(&mut failed, false);
    new_session(&mut failed);
    running(&mut failed);
    let batch = failed
        .ingest(&permission(18, "permission-fail"), &Mapper)
        .expect("deny");
    let token = batch.egress()[0].token().clone();
    failed.commit_mandatory_egress(&token).expect("commit");
    assert_eq!(
        failed.resolve_egress(token, EgressResolution::WriteZeroBytes),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
    );
}

#[path = "cancel_contract_tests.rs"]
mod cancel_contract_tests;

#[path = "decline_publication_tests.rs"]
mod decline_publication_tests;

#[path = "settled_report_tests.rs"]
mod settled_report_tests;

#[test]
fn malformed_and_plus_one_models_fail_closed() {
    let mut malformed = fixture_runtime(10);
    malformed
        .command(AdapterCommand::Initialize)
        .expect("initialize");
    assert!(matches!(
        malformed.ingest(&raw_frame(br#"{"jsonrpc":"2.0","id":1,"id":2}"#), &Mapper),
        Err(AdapterRuntimeError::Frame)
    ));
    assert!(matches!(
        malformed.command(AdapterCommand::Initialize),
        Err(AdapterRuntimeError::Operation)
    ));

    let mut bounded = fixture_runtime(11);
    initialize(&mut bounded, false);
    let command = bounded
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture".into(),
            model: None,
        })
        .expect("session");
    let request = deliver(&mut bounded, command);
    let models = (0..129)
        .map(|index| json!({"modelId":format!("model-{index}"),"name":"Model"}))
        .collect::<Vec<_>>();
    assert!(matches!(
        bounded.ingest(
            &response(
                &request,
                json!({"sessionId":"omp-session","models":{"availableModels":models}}),
            ),
            &Mapper,
        ),
        Err(AdapterRuntimeError::Frame)
    ));
}

#[test]
fn chunks_append_plan_replaces_and_plus_one_poison_clears_snapshots() {
    let mut runtime = fixture_runtime(12);
    initialize(&mut runtime, false);
    new_session(&mut runtime);
    let prompt = running(&mut runtime);
    for delta in ["hel", "lo"] {
        let batch = runtime
            .ingest(
                &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
                    "sessionId":"omp-session-1","update":{"sessionUpdate":"agent_message_chunk",
                    "itemId":"message-chunks","content":{"type":"text","text":delta}}
                }})),
                &Mapper,
            )
            .expect("message chunk");
        if delta == "lo" {
            assert!(matches!(
                batch.events().last(),
                Some(AdapterEvent::Item {
                    payload: NormalizedPayload::Text { text },
                    ..
                }) if text.as_str() == "hello"
            ));
        }
    }
    for (item, first, second, forbidden) in [
        ("split-bearer", "Bearer ", "secret-value", "secret-value"),
        ("split-path", "C:", r"\Users\private", "Users"),
        ("split-key", "sk-", "private", "private"),
    ] {
        for text in [first, second] {
            let batch = runtime
                .ingest(
                    &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
                        "sessionId":"omp-session-1","update":{
                            "sessionUpdate":"agent_message_chunk","itemId":item,
                            "content":{"type":"text","text":text}
                        }
                    }})),
                    &Mapper,
                )
                .expect("split chunk");
            if text == second {
                assert!(!format!("{:?}", batch.events()).contains(forbidden));
            }
        }
    }
    for content in ["First", "Second"] {
        let batch = runtime
            .ingest(
                &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
                    "sessionId":"omp-session-1","update":{"sessionUpdate":"plan","itemId":"plan-full",
                    "entries":[{"status":"completed","content":content}]}
                }})),
                &Mapper,
            )
            .expect("plan snapshot");
        if content == "Second" {
            assert!(matches!(
                batch.events(),
                [AdapterEvent::Item {
                    payload: NormalizedPayload::Plan { summary },
                    ..
                }] if summary.as_str() == "[completed] Second"
            ));
        }
    }
    assert_eq!(runtime.snapshots.len(), 5);
    runtime
        .ingest(
            &response(&prompt, json!({"stopReason":"end_turn"})),
            &Mapper,
        )
        .expect("terminal");
    assert_eq!(runtime.snapshots.len(), 0);

    let mut bounded = fixture_runtime(13);
    initialize(&mut bounded, false);
    new_session(&mut bounded);
    running(&mut bounded);
    bounded
        .ingest(
            &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"omp-session-1","update":{"sessionUpdate":"agent_message_chunk",
                "itemId":"full","content":{"type":"text","text":"x".repeat(262_144)}}
            }})),
            &Mapper,
        )
        .expect("exact bound");
    assert!(matches!(
        bounded.ingest(
            &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"omp-session-1","update":{"sessionUpdate":"agent_message_chunk",
                "itemId":"full","content":{"type":"text","text":"x"}}
            }})),
            &Mapper,
        ),
        Err(AdapterRuntimeError::BatchBytes)
    ));
    assert_eq!(bounded.snapshots.len(), 0);
    assert!(matches!(
        bounded.command(AdapterCommand::Cancel),
        Err(AdapterRuntimeError::Operation)
    ));
}
