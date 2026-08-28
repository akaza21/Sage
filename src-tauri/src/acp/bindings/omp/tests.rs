use serde_json::{Value, json};

use crate::acp::{
    contract::{SageRootId, ToolState},
    intent::{NormalizedPayload, SafeResourceRef, ToolCategory, UnavailableResourceReason},
    limits::LIMITS,
};

use super::{
    AdapterEvent, ArtifactIdentity, CandidateManifest, CuratedOmpBinding, OMP_BUN_MINIMUM,
    OMP_LICENSE, OMP_PACKAGE, OMP_SOURCE_COMMIT, OMP_VERSION, OmpBindingError, OmpPlatform,
    OmpTranslator, Operation, PromptTerminal, ResourceMapper, TranslationError,
};

struct FixtureResources;
struct RejectResources;

impl ResourceMapper for FixtureResources {
    fn map_absolute(&self, path: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        if path != r"C:\fixture\src\lib.rs" {
            return Err(UnavailableResourceReason::Foreign);
        }
        SafeResourceRef::new(
            SageRootId::parse("root-fixture")
                .map_err(|_| UnavailableResourceReason::Unsupported)?,
            "src/lib.rs",
        )
        .map_err(|_| UnavailableResourceReason::Unsupported)
    }
}

impl ResourceMapper for RejectResources {
    fn map_absolute(&self, _path: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

fn identity(path: &str, byte: char, bytes: u64) -> ArtifactIdentity {
    ArtifactIdentity {
        relative_path: path.into(),
        sha256: byte.to_string().repeat(64),
        bytes,
    }
}

fn admitted_candidate(read_only: bool) -> (CuratedOmpBinding, CandidateManifest) {
    let entrypoint = identity("omp/bin/omp", 'a', 100);
    let runtime = identity("bun/bun.exe", 'b', 200);
    let addons = vec![identity("omp/addons/parser.node", 'c', 50)];
    (
        CuratedOmpBinding {
            platform: OmpPlatform::WindowsX64,
            acp_protocol_version: "fixture-acp-v1".into(),
            bun_version: OMP_BUN_MINIMUM.into(),
            entrypoint: entrypoint.clone(),
            runtime: runtime.clone(),
            native_addons: addons.clone(),
        },
        CandidateManifest {
            platform: OmpPlatform::WindowsX64,
            package_name: OMP_PACKAGE.into(),
            package_version: OMP_VERSION.into(),
            source_commit: OMP_SOURCE_COMMIT.into(),
            license: OMP_LICENSE.into(),
            acp_protocol_version: "fixture-acp-v1".into(),
            bun_version: OMP_BUN_MINIMUM.into(),
            entrypoint,
            runtime,
            native_addons: addons,
            fixed_args: vec!["acp".into()],
            isolated_state: true,
            isolated_auth: true,
            ambient_extensions_disabled: true,
            ambient_configuration_disabled: true,
            dynamic_downloads_disabled: true,
            browser_subprocess_lsp_dap_disabled: true,
            offline_and_telemetry_reviewed: true,
            read_only_authority_verified: read_only,
            native_acp_ndjson_verified: true,
            disconnect_teardown_verified: true,
        },
    )
}

fn translator(epoch: u64) -> OmpTranslator {
    let (curated, candidate) = admitted_candidate(true);
    let binding = curated.validate_candidate(&candidate).unwrap();
    OmpTranslator::new(epoch, binding.capabilities())
}

fn rpc_result(id: &str, result: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"result":result})).unwrap()
}

fn initialize(translator: &mut OmpTranslator, load: bool) {
    let request = translator.begin(Operation::Initialize).unwrap().remove(0);
    let events = translator
        .ingest(
            &rpc_result(
                request.request_id.as_deref().unwrap(),
                json!({"protocolVersion":1,"agentCapabilities":{"loadSession":load}}),
            ),
            &FixtureResources,
        )
        .unwrap();
    assert_eq!(
        events,
        vec![AdapterEvent::Initialized {
            protocol_version: 1,
            load_session: load,
        }]
    );
}

fn new_session(translator: &mut OmpTranslator) {
    let request = translator
        .begin(Operation::NewSession {
            native_cwd: r"C:\fixture".into(),
            root_count: 1,
        })
        .unwrap()
        .remove(0);
    let events = translator
        .ingest(
            &rpc_result(
                request.request_id.as_deref().unwrap(),
                json!({
                    "sessionId":"omp-session-1",
                    "models":{
                        "currentModelId":"model-a",
                        "availableModels":[{"modelId":"model-a","name":"Model A"}]
                    }
                }),
            ),
            &FixtureResources,
        )
        .unwrap();
    assert!(
        matches!(events.as_slice(), [AdapterEvent::SessionReady { models, .. }] if models.len() == 1 && models[0].is_current)
    );
}

fn begin_prompt(translator: &mut OmpTranslator) -> String {
    translator
        .begin(Operation::Prompt {
            turn_key: "turn-1".into(),
            message: "Inspect without tools.".into(),
        })
        .unwrap()
        .remove(0)
        .request_id
        .unwrap()
}

#[test]
fn exact_manifest_is_fail_closed_and_never_admits_launch() {
    let (curated, mut candidate) = admitted_candidate(true);
    let binding = curated.validate_candidate(&candidate).unwrap();
    let capabilities = binding.capabilities();
    assert!(capabilities.initialize() && capabilities.new_session());
    assert!(capabilities.prompt() && capabilities.cancel());
    assert!(capabilities.load_session_ceiling());
    assert!(!capabilities.resume_session() && !capabilities.close_session());
    assert!(!capabilities.multi_root() && !capabilities.write_tools());
    assert_eq!(
        binding.require_launch_admission(),
        Err(OmpBindingError::ArtifactGraphNotAdmitted)
    );

    candidate.browser_subprocess_lsp_dap_disabled = false;
    assert_eq!(
        curated.validate_candidate(&candidate),
        Err(OmpBindingError::IdentityMismatch("broad process authority"))
    );
    let (curated, candidate) = admitted_candidate(false);
    assert_eq!(
        curated.validate_candidate(&candidate),
        Err(OmpBindingError::ReadOnlyUnproven)
    );
}

#[test]
fn artifact_paths_addons_and_bun_are_mechanically_bounded() {
    let platforms = [
        OmpPlatform::WindowsX64,
        OmpPlatform::LinuxX64,
        OmpPlatform::LinuxArm64,
        OmpPlatform::MacOsX64,
        OmpPlatform::MacOsArm64,
    ];
    assert_eq!(platforms.len(), 5);
    for path in ["../omp", "omp/./bin", "C:/omp/bin", "/omp/bin"] {
        let (mut curated, candidate) = admitted_candidate(true);
        curated.entrypoint.relative_path = path.into();
        assert!(matches!(
            curated.validate_candidate(&candidate),
            Err(OmpBindingError::InvalidCuratedMetadata(_))
        ));
    }
    let (mut curated, mut candidate) = admitted_candidate(true);
    curated.bun_version = "1.3.13".into();
    candidate.bun_version = curated.bun_version.clone();
    assert_eq!(
        curated.validate_candidate(&candidate),
        Err(OmpBindingError::InvalidCuratedMetadata("Bun version"))
    );
    let (mut curated, mut candidate) = admitted_candidate(true);
    curated.native_addons.push(curated.native_addons[0].clone());
    candidate.native_addons = curated.native_addons.clone();
    assert_eq!(
        curated.validate_candidate(&candidate),
        Err(OmpBindingError::InvalidCuratedMetadata(
            "duplicate addon path"
        ))
    );
}

#[test]
fn initialize_stays_deny_only_and_incoming_sage_browse_fails_closed() {
    let mut translator = translator(11);
    let request = translator.begin(Operation::Initialize).unwrap().remove(0);
    let value: Value = serde_json::from_str(request.line.trim()).expect("initialize");
    assert_eq!(
        value["params"]["clientCapabilities"]["fs"]["readTextFile"],
        false
    );
    assert_eq!(
        value["params"]["clientCapabilities"]["fs"]["writeTextFile"],
        false
    );
    assert_eq!(value["params"]["clientCapabilities"]["terminal"], false);
    assert!(
        value["params"]["clientCapabilities"]
            .get("availableTools")
            .is_none()
    );
    translator
        .ingest(
            &rpc_result(
                request.request_id.as_deref().unwrap(),
                json!({"protocolVersion":1,"agentCapabilities":{"loadSession":false}}),
            ),
            &FixtureResources,
        )
        .unwrap();
    new_session(&mut translator);
    begin_prompt(&mut translator);
    assert!(matches!(
        translator.ingest(
            br#"{"jsonrpc":"2.0","id":"browse-1","method":"sage.browse","params":{"url":"http://127.0.0.1:5173/"}}"#,
            &FixtureResources,
        ),
        Err(TranslationError::UnexpectedMethod)
    ));
}

#[test]
fn initialize_new_load_and_model_discovery_follow_negotiated_capabilities() {
    let mut adapter = translator(1);
    initialize(&mut adapter, true);
    new_session(&mut adapter);
    let request = adapter
        .begin(Operation::LoadSession {
            external_session_id: "omp-session-1".into(),
            native_cwd: r"C:\fixture".into(),
            root_count: 1,
        })
        .unwrap();
    assert!(request[0].line.contains("session/load"));
    let request_id = request[0].request_id.as_deref().unwrap();
    assert!(matches!(
        adapter.ingest(
            &rpc_result(request_id, json!({"sessionId":"foreign-session"})),
            &FixtureResources,
        ),
        Err(TranslationError::Correlation)
    ));

    let mut no_load = translator(2);
    initialize(&mut no_load, false);
    assert!(matches!(
        no_load.begin(Operation::LoadSession {
            external_session_id: "session".into(),
            native_cwd: "/fixture".into(),
            root_count: 1,
        }),
        Err(TranslationError::UnsupportedOperation("session/load"))
    ));
}

#[test]
fn read_only_fixture_normalizes_items_and_denies_broad_tool_authority() {
    let mut translator = translator(3);
    initialize(&mut translator, false);
    new_session(&mut translator);
    let prompt_id = begin_prompt(&mut translator);
    let mut events = Vec::new();
    for line in include_str!("../../../../testdata/acp/omp/read_only_turn.jsonl").lines() {
        events.extend(
            translator
                .ingest(line.as_bytes(), &FixtureResources)
                .unwrap(),
        );
    }
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::ThoughtSummary { .. },
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::Plan { .. },
            ..
        }
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AdapterEvent::ToolAuthorityDenied { .. }))
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AdapterEvent::CancelDispatchRequired))
            .count(),
        1
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item { payload: NormalizedPayload::FileChangeDiff { resource, additions: Some(2), deletions: Some(1), .. }, .. }
        if matches!(resource, crate::acp::intent::SafeResourceLocation::Resource { resource }
            if resource.relative_path.as_str() == "src/lib.rs")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::Unsupported { .. },
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::Usage {
                input_tokens: 12,
                output_tokens: 7
            },
            ..
        }
    )));
    assert!(!format!("{events:?}").contains(r"C:\fixture"));

    let terminal = translator
        .ingest(
            &rpc_result(&prompt_id, json!({"stopReason":"end_turn"})),
            &FixtureResources,
        )
        .unwrap();
    assert_eq!(
        terminal,
        vec![AdapterEvent::TurnTerminal {
            turn_key: "turn-1".into(),
            terminal: PromptTerminal::Completed,
        }]
    );
}

#[test]
fn permission_denial_waits_for_durable_commit_and_is_idempotent() {
    let mut translator = translator(4);
    initialize(&mut translator, false);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    let request = br#"{"jsonrpc":"2.0","id":17,"method":"session/request_permission","params":{"sessionId":"omp-session-1","toolCall":{"toolCallId":"tool-permission","kind":"execute","title":"bash"},"options":[{"optionId":"allow","kind":"allow_once"}]}}"#;
    let events = translator.ingest(request, &FixtureResources).unwrap();
    assert!(matches!(
        events.as_slice(),
        [
            AdapterEvent::Item {
                tool_state: Some(ToolState::WaitingForApproval),
                ..
            },
            AdapterEvent::PermissionDeclinePending { .. }
        ]
    ));
    let response = translator.permission_decline_committed("17").unwrap();
    assert!(response.outbound.line.contains("\"id\":17"));
    assert!(response.outbound.line.contains("\"outcome\":\"cancelled\""));
    assert!(matches!(
        response.event,
        AdapterEvent::Item {
            tool_state: Some(ToolState::Declined),
            ..
        }
    ));
    let duplicate = translator.permission_decline_committed("17").unwrap();
    assert_eq!(duplicate.outbound.line, response.outbound.line);
    assert_eq!(duplicate.event, AdapterEvent::IgnoredDuplicate);
}

#[test]
fn cancellation_terminal_duplicates_and_late_updates_are_race_safe() {
    let mut translator = translator(5);
    initialize(&mut translator, false);
    new_session(&mut translator);
    let prompt_id = begin_prompt(&mut translator);
    let cancel = translator.begin(Operation::Cancel).unwrap();
    assert_eq!(cancel.len(), 1);
    assert!(cancel[0].line.contains("session/cancel"));
    assert!(translator.begin(Operation::Cancel).unwrap().is_empty());
    let terminal_frame = rpc_result(&prompt_id, json!({"stopReason":"cancelled"}));
    let terminal = translator
        .ingest(&terminal_frame, &FixtureResources)
        .unwrap();
    assert!(matches!(
        terminal.as_slice(),
        [AdapterEvent::TurnTerminal {
            terminal: PromptTerminal::Cancelled,
            ..
        }]
    ));
    assert_eq!(
        translator
            .ingest(&terminal_frame, &FixtureResources)
            .unwrap(),
        vec![AdapterEvent::IgnoredDuplicate]
    );
    let late = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"late"}}}}"#;
    assert_eq!(
        translator.ingest(late, &FixtureResources).unwrap(),
        vec![AdapterEvent::IgnoredLate]
    );
}

#[test]
fn unsupported_lifecycle_multi_root_and_noncanonical_native_paths_fail_closed() {
    let mut translator = translator(6);
    initialize(&mut translator, false);
    for operation in [Operation::ResumeSession, Operation::CloseSession] {
        assert!(matches!(
            translator.begin(operation),
            Err(TranslationError::UnsupportedOperation(_))
        ));
    }
    assert!(matches!(
        translator.begin(Operation::NewSession {
            native_cwd: "/fixture".into(),
            root_count: 2
        }),
        Err(TranslationError::UnsupportedOperation("multi-root"))
    ));
    assert!(matches!(
        translator.begin(Operation::NewSession {
            native_cwd: "/fixture/../escape".into(),
            root_count: 1
        }),
        Err(TranslationError::NativePath)
    ));
}

#[test]
fn malformed_duplicate_key_deep_foreign_and_unknown_frames_are_rejected() {
    let mut translator = translator(7);
    initialize(&mut translator, false);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    let fixture = include_str!("../../../../testdata/acp/omp/malformed.jsonl");
    let lines = fixture.lines().collect::<Vec<_>>();
    assert!(matches!(
        translator.ingest(lines[0].as_bytes(), &FixtureResources),
        Err(TranslationError::Wire(_))
    ));
    assert!(matches!(
        translator.ingest(lines[1].as_bytes(), &FixtureResources),
        Err(TranslationError::Correlation)
    ));
    assert!(matches!(
        translator.ingest(lines[2].as_bytes(), &FixtureResources),
        Err(TranslationError::UnexpectedMethod)
    ));
    let deep = format!("{}0{}", "[".repeat(40), "]".repeat(40));
    assert!(matches!(
        translator.ingest(deep.as_bytes(), &FixtureResources),
        Err(TranslationError::Wire(_))
    ));
}

#[test]
fn normalized_text_redacts_secrets_and_absolute_paths() {
    let mut translator = translator(8);
    initialize(&mut translator, false);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    let event = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"See C:\\Users\\private, sk-secret, and Bearer token-secret"}}}}"#;
    let events = translator.ingest(event, &FixtureResources).unwrap();
    let rendered = format!("{events:?}");
    assert!(!rendered.contains("Users"));
    assert!(!rendered.contains("sk-secret"));
    assert!(!rendered.contains("token-secret"));
    assert!(rendered.contains("[redacted]"));
}

#[test]
fn pending_request_ledger_is_exactly_bounded_at_sixteen_plus_one() {
    let mut translator = translator(9);
    for _ in 0..16 {
        translator.issue_probe().unwrap();
    }
    assert_eq!(translator.pending_count(), 16);
    assert!(matches!(
        translator.issue_probe(),
        Err(TranslationError::RequestCapacity)
    ));
}

#[test]
fn frame_model_tool_and_permission_limits_reject_plus_one() {
    let oversized = vec![b'x'; LIMITS.wire.frame_bytes as usize + 1];
    assert!(matches!(
        translator(10).ingest(&oversized, &FixtureResources),
        Err(TranslationError::Wire(_))
    ));

    let mut model_adapter = translator(11);
    initialize(&mut model_adapter, false);
    let request = model_adapter
        .begin(Operation::NewSession {
            native_cwd: "/fixture".into(),
            root_count: 1,
        })
        .unwrap()
        .remove(0);
    let models = (0..129)
        .map(|index| json!({"modelId":format!("model-{index}"),"name":"Model"}))
        .collect::<Vec<_>>();
    assert!(matches!(
        model_adapter.ingest(
            &rpc_result(
                request.request_id.as_deref().unwrap(),
                json!({"sessionId":"omp-session-1","models":{"availableModels":models}}),
            ),
            &FixtureResources,
        ),
        Err(TranslationError::Limit("model count"))
    ));

    let mut tool_adapter = translator(12);
    initialize(&mut tool_adapter, false);
    new_session(&mut tool_adapter);
    begin_prompt(&mut tool_adapter);
    for index in 0..512 {
        let frame = serde_json::to_vec(&json!({
            "jsonrpc":"2.0","method":"session/update",
            "params":{"sessionId":"omp-session-1","update":{
                "sessionUpdate":"tool_call","toolCallId":format!("tool-{index}"),
                "kind":"read","status":"pending"
            }}
        }))
        .unwrap();
        tool_adapter.ingest(&frame, &FixtureResources).unwrap();
    }
    let plus_one = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"tool_call","toolCallId":"tool-plus-one","kind":"read"}}}"#;
    assert!(matches!(
        tool_adapter.ingest(plus_one, &FixtureResources),
        Err(TranslationError::Limit("tools per Turn"))
    ));

    let mut permission_adapter = translator(13);
    initialize(&mut permission_adapter, false);
    new_session(&mut permission_adapter);
    begin_prompt(&mut permission_adapter);
    for index in 0..8 {
        let frame = serde_json::to_vec(&json!({
            "jsonrpc":"2.0","id":index,"method":"session/request_permission",
            "params":{"sessionId":"omp-session-1","toolCall":{
                "toolCallId":format!("permission-tool-{index}"),"kind":"execute"
            },"options":[]}
        }))
        .unwrap();
        permission_adapter
            .ingest(&frame, &FixtureResources)
            .unwrap();
    }
    let plus_one = br#"{"jsonrpc":"2.0","id":9,"method":"session/request_permission","params":{"sessionId":"omp-session-1","toolCall":{"toolCallId":"permission-plus-one","kind":"execute"},"options":[]}}"#;
    assert!(matches!(
        permission_adapter.ingest(plus_one, &FixtureResources),
        Err(TranslationError::Limit("permission requests"))
    ));
}

#[test]
fn typed_tool_categories_and_errors_exclude_raw_arguments() {
    let mut translator = translator(14);
    initialize(&mut translator, false);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    let cases = [
        ("search", "Search", ToolCategory::Search),
        ("fetch", "Browser", ToolCategory::Web),
        ("other", "MCP provider", ToolCategory::Mcp),
        ("other", "subagent", ToolCategory::Subagent),
    ];
    for (index, (kind, title, expected)) in cases.into_iter().enumerate() {
        let frame = serde_json::to_vec(&json!({
            "jsonrpc":"2.0","method":"session/update",
            "params":{"sessionId":"omp-session-1","update":{
                "sessionUpdate":"tool_call","toolCallId":format!("category-{index}"),
                "kind":kind,"title":title,"rawInput":{
                    "command":"remove C:\\Users\\private\\secret.txt","token":"sk-secret"
                }
            }}
        }))
        .unwrap();
        let events = translator.ingest(&frame, &FixtureResources).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            AdapterEvent::Item {
                payload: NormalizedPayload::ToolCall { category, input_summary: None, .. },
                ..
            } if *category == expected
        )));
        let rendered = format!("{events:?}");
        assert!(!rendered.contains("Users"));
        assert!(!rendered.contains("sk-secret"));
    }
    let error = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"error","itemId":"error-1","message":"Bearer secret C:\\Users\\private","retryable":true}}}"#;
    let events = translator.ingest(error, &FixtureResources).unwrap();
    assert!(matches!(
        events.as_slice(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::Error {
                retryable: true,
                ..
            },
            ..
        }]
    ));
    assert!(!format!("{events:?}").contains("secret"));
}

#[test]
fn failed_resource_mapping_emits_a_typed_nonclickable_location() {
    let mut adapter = translator(15);
    initialize(&mut adapter, false);
    new_session(&mut adapter);
    begin_prompt(&mut adapter);
    let start = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"tool_call","toolCallId":"mapped-tool","kind":"edit"}}}"#;
    adapter.ingest(start, &FixtureResources).unwrap();
    let diff = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"mapped-tool","kind":"edit","status":"completed","content":[{"type":"diff","path":"C:\\fixture\\src\\lib.rs"}]}}}"#;
    let mapped = adapter.ingest(diff, &RejectResources).unwrap();
    assert!(mapped.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            tool_state: Some(ToolState::Completed),
            payload: NormalizedPayload::ToolUpdateResult { .. },
            ..
        }
    )));
    assert!(mapped.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::FileChangeDiff {
                resource: crate::acp::intent::SafeResourceLocation::Unavailable {
                    reason: UnavailableResourceReason::Foreign,
                    ..
                },
                ..
            },
            ..
        }
    )));

    let mut fresh = translator(16);
    initialize(&mut fresh, false);
    new_session(&mut fresh);
    begin_prompt(&mut fresh);
    let malformed = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"plan"}}}"#;
    assert!(matches!(
        fresh.ingest(malformed, &FixtureResources),
        Err(TranslationError::MalformedFrame)
    ));
    let valid = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ok"}}}}"#;
    assert!(matches!(
        fresh.ingest(valid, &FixtureResources).unwrap().as_slice(),
        [AdapterEvent::TurnRunning { .. }, AdapterEvent::Item { .. }]
    ));
}
