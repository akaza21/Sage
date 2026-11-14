use std::{
    fs, io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::AsyncWrite;

use serde_json::{Value, json};
use uuid::Uuid;

use crate::acp::{
    contract::{
        ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageRootId,
        ToolState,
    },
    intent::{
        COMMAND_RESOURCE_LIMIT, NormalizedPayload, SafeResourceLocation, SafeResourceRef,
        ToolCategory, UnavailableResourceReason,
    },
    read_text_file::{
        DiskAllowedState, ReadTextFileService, ReadTextFlushAck, ReadTextFlushState, ReadTextOwner,
    },
    runtime::read_text_file::{DeferredReadTextOwnerResolver, ExpectedOwnerScope},
    transport::DeadlineAsyncWriter,
};
use sage_core::limits::LIMITS;

use super::{
    AdapterEvent, ArtifactIdentity, CandidateManifest, CuratedOmpBinding, OMP_BUN_MINIMUM,
    OMP_LICENSE, OMP_PACKAGE, OMP_SOURCE_COMMIT, OMP_VERSION, OmpBindingError, OmpPlatform,
    OmpRuntime, OmpTranslator, Operation, PromptTerminal, ResourceMapper, TranslationError,
};

struct P02Capture(Vec<u8>);
impl AsyncWrite for P02Capture {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0.extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct P02Owner(ReadTextOwner);
impl DeferredReadTextOwnerResolver for P02Owner {
    fn owner_for_external_session(
        &self,
        session: &str,
        expected: &ExpectedOwnerScope,
    ) -> Option<ReadTextOwner> {
        (session == "omp-session-1" && expected.owner == self.0).then(|| self.0.clone())
    }
}

#[test]
fn p02_omp_real_ingress_preflights_routes_service_and_preserves_numeric_id() {
    let root = std::env::temp_dir().join(format!("sage-p02-omp-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("file.txt");
    fs::write(&file, "one\ntwo\n").unwrap();
    let database = crate::persistence::Database::open(":memory:".as_ref()).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let owner = ReadTextOwner {
        project_id: SageProjectId::parse(&project.id).unwrap(),
        project_generation: ProjectGeneration::new(1).unwrap(),
        chat_id: SageChatId::parse("chat-p02").unwrap(),
        session_id: SageAgentSessionId::parse("session-omp-runtime").unwrap(),
        runtime_epoch: RuntimeEpoch::new(3).unwrap(),
    };
    let service = ReadTextFileService::new(database);
    let expected = ExpectedOwnerScope {
        binding_digest: [11; 32],
        owner: owner.clone(),
    };
    let (curated, candidate) = admitted_candidate(true);
    let mut runtime = OmpRuntime::fixture(
        curated
            .validate_candidate(&candidate)
            .unwrap()
            .capabilities(),
        RuntimeEpoch::new(3).unwrap(),
        SageAgentSessionId::parse("session-omp-runtime").unwrap(),
    );
    runtime.external_session =
        Some(crate::acp::intent::ExternalId::new("sessionId", "omp-session-1").unwrap());
    let path = serde_json::to_string(&file.display().to_string()).unwrap();
    let request = format!(
        r#"{{"jsonrpc":"2.0","id":7,"method":"fs/read_text_file","params":{{"sessionId":"omp-session-1","path":{},"line":2,"limit":1,"_meta":{{"opaque":true}}}}}}"#,
        path
    );
    let handoff = runtime
        .ingest_deferred_read_text_file(
            request.as_bytes(),
            &expected,
            &P02Owner(owner.clone()),
            &service,
        )
        .unwrap();
    service
        .acknowledge(ReadTextFlushAck {
            owner: handoff.begin.event.owner.clone(),
            resource: handoff.begin.event.resource.clone(),
            expected_editor_revision: handoff.begin.event.expected_editor_revision,
            token: handoff.begin.event.token.clone().into(),
            state: ReadTextFlushState::Classify {
                state: crate::acp::read_text_file::ReadTextClassification::NotOpen,
                editor_revision: None,
            },
        })
        .unwrap();
    service
        .acknowledge(ReadTextFlushAck {
            owner: handoff.begin.event.owner.clone(),
            resource: handoff.begin.event.resource.clone(),
            expected_editor_revision: handoff.begin.event.expected_editor_revision,
            token: handoff.begin.event.token.clone().into(),
            state: ReadTextFlushState::DiskAllowed {
                state: DiskAllowedState::NotOpen,
                editor_revision: None,
            },
        })
        .unwrap();
    let mut egress = DeadlineAsyncWriter::new(P02Capture(Vec::new()));
    let receipt = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(
            handoff
                .begin
                .receiver
                .blocking_receive()
                .unwrap()
                .deliver_response(&handoff.request_id, &mut egress),
        )
        .unwrap();
    let bytes = egress.into_inner().0;
    assert!(
        serde_json::to_string(&receipt.receipt().payload)
            .unwrap()
            .contains("outcome=success")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes[..bytes.len() - 1]).unwrap(),
        json!({"jsonrpc":"2.0","id":7,"result":{"content":"two\n"}})
    );
    assert!(bytes.ends_with(b"\n"));
    for malformed in [
        request.replace("\"id\":7", "\"id\":1.5"),
        request.replace("\"id\":7", "\"id\":9223372036854775808"),
        request.replace("\"_meta\":{\"opaque\":true}", "\"_meta\":true"),
        request.replace("\"omp-session-1\"", "\"foreign-session\""),
    ] {
        assert!(matches!(
            runtime.ingest_deferred_read_text_file(
                malformed.as_bytes(),
                &expected,
                &P02Owner(owner.clone()),
                &service
            ),
            Err(crate::acp::bindings::adapter::AdapterRuntimeError::Frame)
        ));
    }
    let mut wrong_binding = expected.clone();
    wrong_binding.binding_digest = [12; 32];
    let mut wrong_project = expected.clone();
    wrong_project.owner.project_id = SageProjectId::parse("project-other").unwrap();
    let mut wrong_generation = expected.clone();
    wrong_generation.owner.project_generation = ProjectGeneration::new(2).unwrap();
    let mut wrong_chat = expected.clone();
    wrong_chat.owner.chat_id = SageChatId::parse("chat-other").unwrap();
    let mut wrong_session = expected.clone();
    wrong_session.owner.session_id = SageAgentSessionId::parse("session-other").unwrap();
    let mut wrong_epoch = expected.clone();
    wrong_epoch.owner.runtime_epoch = RuntimeEpoch::new(4).unwrap();
    for scope in [
        wrong_binding,
        wrong_project,
        wrong_generation,
        wrong_chat,
        wrong_session,
        wrong_epoch,
    ] {
        assert!(
            runtime
                .ingest_deferred_read_text_file(
                    request.as_bytes(),
                    &scope,
                    &P02Owner(owner.clone()),
                    &service
                )
                .is_err()
        );
    }
    fs::remove_dir_all(root).unwrap();
}

struct FixtureResources;
struct RejectResources;
struct LongResources;

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

impl ResourceMapper for LongResources {
    fn map_absolute(&self, path: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        SafeResourceRef::new(
            SageRootId::parse("root-fixture")
                .map_err(|_| UnavailableResourceReason::Unsupported)?,
            path.trim_start_matches('/'),
        )
        .map_err(|_| UnavailableResourceReason::Unsupported)
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

#[test]
fn omp_mcp_wire_keeps_empty_array_and_gates_unadvertised_transport() {
    let mut empty = translator(1);
    initialize(&mut empty, false);
    let wire = empty
        .begin(Operation::NewSession {
            native_cwd: r"C:\fixture".into(),
            root_count: 1,
        })
        .unwrap()
        .remove(0)
        .line;
    assert!(wire.contains("\"mcpServers\":[]"), "{wire}");

    let (curated, candidate) = admitted_candidate(true);
    let binding = curated.validate_candidate(&candidate).unwrap();
    let admission = crate::acp::mcp::McpAdmission::for_test(vec![
        crate::acp::mcp::McpServerDescriptor::stdio_for_test(
            "files",
            r"C:\tools\mcp.exe",
            &[],
            vec![],
        ),
        crate::acp::mcp::McpServerDescriptor::sse_for_test("remote", "https://example.com/events"),
    ])
    .unwrap();
    let mut injected = OmpTranslator::with_mcp(2, binding.capabilities(), admission);
    initialize(&mut injected, false);
    let wire = injected
        .begin(Operation::NewSession {
            native_cwd: r"C:\fixture".into(),
            root_count: 1,
        })
        .unwrap()
        .remove(0)
        .line;
    let value: Value = serde_json::from_str(&wire).unwrap();
    assert_eq!(
        value["params"]["mcpServers"],
        json!([{
            "name":"files","command":r"C:\tools\mcp.exe","args":[],"env":[]
        }])
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
fn initialize_advertises_p02_read_conformance_and_browse_extensions_fail_closed() {
    let mut translator = translator(11);
    let request = translator.begin(Operation::Initialize).unwrap().remove(0);
    let value: Value = serde_json::from_str(request.line.trim()).expect("initialize");
    assert_eq!(
        value["params"]["clientCapabilities"]["fs"]["readTextFile"],
        true
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
    assert!(value["params"]["clientCapabilities"].get("_meta").is_none());
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
    for method in ["_sage/browse", "sage.browse", "sage_browse"] {
        let frame = format!(
            r#"{{"jsonrpc":"2.0","id":"browse-1","method":"{method}","params":{{"url":"http://127.0.0.1:5173/"}}}}"#
        );
        assert!(matches!(
            translator.ingest(frame.as_bytes(), &FixtureResources),
            Err(TranslationError::UnexpectedMethod)
        ));
    }
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
                output_tokens: 7,
                context_window: None,
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
fn permission_receipt_names_the_typed_tool_operation() {
    let mut translator = translator(400);
    initialize(&mut translator, false);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    let request = br#"{"jsonrpc":"2.0","id":17,"method":"session/request_permission","params":{"sessionId":"omp-session-1","toolCall":{"toolCallId":"tool-permission","kind":"execute","title":"PowerShell","rawInput":{"command":"Get-Location"}},"options":[]}}"#;
    let events = translator.ingest(request, &FixtureResources).unwrap();
    assert!(events.iter().any(|event| matches!(event,
        AdapterEvent::Item {
            payload: NormalizedPayload::PermissionRequest { category: ToolCategory::Execute, summary, .. }, ..
        } if summary.as_str() == "Run command: Get-Location"
    )));
}

#[test]
fn permission_command_summaries_redact_sensitive_forms() {
    for (index, (command, private)) in [
        ("API_KEY=hunter2", "hunter2"),
        ("MY_SECRET=hunter2", "hunter2"),
        ("--auth-token=abc123", "abc123"),
        ("--password abc123", "abc123"),
        ("-Password abc123", "abc123"),
        ("--pass abc123", "abc123"),
        ("--pw=abc123", "abc123"),
        ("AUTH=Bearer abc123", "abc123"),
        ("Bearer abc123", "abc123"),
        ("Bearer:abc123", "abc123"),
        ("Basic abc123", "abc123"),
        ("Basic:abc123", "abc123"),
        ("mysql -u x -p secret", "secret"),
        ("mysql -psecret", "secret"),
        ("sshpass -p secret", "secret"),
        ("-H 'Authorization: Token abc123'", "abc123"),
        ("Authorization: abc123", "abc123"),
        ("Proxy-Authorization: abc123", "abc123"),
        ("Cookie: abc123", "abc123"),
        ("X-Api-Key: abc123", "abc123"),
        ("X-Auth-Token: abc123", "abc123"),
        ("Authorization: 'abc123'", "abc123"),
        ("X-Api-Key: \"abc123\"", "abc123"),
        ("Authorization : 'abc123'", "abc123"),
        ("mysql -p\"my secret\"", "my secret"),
        ("mysql -pmy\\ secret", "my\\ secret"),
        ("mysql -pmy` secret", "my` secret"),
        ("curl -u user:pass https://example.test", "user:pass"),
        ("curl --user user:pass https://example.test", "user:pass"),
        (
            "curl --proxy-user user:pass https://example.test",
            "user:pass",
        ),
        ("curl -uuser:pass https://example.test", "user:pass"),
        ("curl --user=user:pass https://example.test", "user:pass"),
        ("$env:API_KEY = 'hunter2'", "hunter2"),
        ("export API_KEY = hunter2", "hunter2"),
        (
            "--password 'mixed \"quote\" secret'",
            "mixed \"quote\" secret",
        ),
        ("sk-abc123", "sk-abc123"),
        ("ghp_abc123", "ghp_abc123"),
        ("xoxb-abc123", "xoxb-abc123"),
        ("https://user:pass@example.com/", "user:pass"),
        ("git clone https://tok@host.test/repo", "tok"),
        ("git clone https://x-access-token:abc@host.test/repo", "abc"),
        ("git clone https://oauth2:abc@host.test/repo", "abc"),
        ("--with-token < secret.txt", "secret.txt"),
        ("$env:API_KEY='hunter2'", "hunter2"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut translator = translator(500 + index as u64);
        initialize(&mut translator, false);
        new_session(&mut translator);
        begin_prompt(&mut translator);
        let frame = serde_json::to_vec(&json!({
            "jsonrpc":"2.0", "id":index + 1, "method":"session/request_permission",
            "params":{"sessionId":"omp-session-1","toolCall":{
                "toolCallId":"tool-permission", "kind":"execute", "title":"PowerShell",
                "rawInput":{"command":command}
            },"options":[]}
        }))
        .unwrap();
        let events = translator.ingest(&frame, &FixtureResources).unwrap();
        let shown = events
            .iter()
            .find_map(|event| match event {
                AdapterEvent::Item {
                    payload: NormalizedPayload::PermissionRequest { summary, .. },
                    ..
                } => Some(summary.as_str()),
                _ => None,
            })
            .expect("permission summary");
        assert!(!shown.contains(private), "{command}: {shown}");
        assert!(shown.contains("[redacted]"), "{command}: {shown}");
    }
    let mut translator = translator(600);
    initialize(&mut translator, false);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    let request = serde_json::to_vec(&json!({
        "jsonrpc":"2.0", "id":99, "method":"session/request_permission",
        "params":{"sessionId":"omp-session-1","toolCall":{
            "toolCallId":"tool-bidi", "kind":"execute", "title":"PowerShell",
            "rawInput":{"command":"Get-\u{202e}Content\u{2069}"}
        },"options":[]}
    }))
    .unwrap();
    let events = translator.ingest(&request, &FixtureResources).unwrap();
    assert!(events.iter().any(|event| matches!(event,
        AdapterEvent::Item { payload: NormalizedPayload::PermissionRequest { summary, .. }, .. }
        if summary.as_str() == "Run command: Get-Content"
    )));
}

#[test]
fn embedded_absolute_paths_do_not_reach_omp_permission_summaries() {
    for (index, command) in [
        "file:///C:/secret",
        "note:/home/secret",
        "mysql -p'unbalanced",
        "cat <<EOF\nhunter2\nEOF",
        "cat <<'EOF'\nhunter2\nEOF",
        "@'hunter2'@",
    ]
    .into_iter()
    .enumerate()
    {
        let mut translator = translator(700 + index as u64);
        initialize(&mut translator, false);
        new_session(&mut translator);
        begin_prompt(&mut translator);
        let frame = serde_json::to_vec(&json!({
            "jsonrpc":"2.0", "id":index + 1, "method":"session/request_permission",
            "params":{"sessionId":"omp-session-1","toolCall":{
                "toolCallId":"tool-permission", "kind":"execute", "title":"PowerShell",
                "rawInput":{"command":command}
            },"options":[]}
        }))
        .unwrap();
        let events = translator.ingest(&frame, &FixtureResources).unwrap();
        let shown = events
            .iter()
            .find_map(|event| match event {
                AdapterEvent::Item {
                    payload: NormalizedPayload::PermissionRequest { summary, .. },
                    ..
                } => Some(summary.as_str()),
                _ => None,
            })
            .expect("permission summary");
        assert!(!shown.contains("secret"), "{command}: {shown}");
        assert!(!shown.contains("/home/"), "{command}: {shown}");
        assert_eq!(
            shown,
            "command hidden: contains content Sage will not display; review before approving"
        );
    }
}

#[test]
fn permission_without_command_keeps_its_title_but_admits_missing_details() {
    let mut translator = translator(401);
    initialize(&mut translator, false);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    let request = br#"{"jsonrpc":"2.0","id":18,"method":"session/request_permission","params":{"sessionId":"omp-session-1","toolCall":{"toolCallId":"tool-edit","kind":"edit","title":"Write README"},"options":[]}}"#;
    let events = translator.ingest(request, &FixtureResources).unwrap();
    assert!(events.iter().any(|event| matches!(event,
        AdapterEvent::Item {
            payload: NormalizedPayload::PermissionRequest { summary, .. }, ..
        } if summary.as_str() == "Write README requested permission; operation details unavailable"
    )));
}

#[test]
fn harness_refusal_has_a_visible_reason() {
    let mut translator = translator(402);
    initialize(&mut translator, false);
    new_session(&mut translator);
    let prompt_id = begin_prompt(&mut translator);
    let events = translator
        .ingest(
            &rpc_result(&prompt_id, json!({ "stopReason": "refusal" })),
            &FixtureResources,
        )
        .unwrap();
    assert!(events.iter().any(|event| matches!(event,
        AdapterEvent::Item {
            payload: NormalizedPayload::Error { message, .. }, ..
        } if message.as_str() == "Declined by OpenCode"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::TurnTerminal {
            terminal: PromptTerminal::Failed,
            ..
        }
    )));
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
        ("fetch", "Browser", ToolCategory::Fetch),
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
    let diff = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"mapped-tool","kind":"edit","status":"completed","content":[{"type":"diff","path":"C:\\fixture\\src\\lib.rs","oldText":"old\n","newText":"new\n"}]}}}"#;
    let mapped = adapter.ingest(diff, &RejectResources).unwrap();
    assert!(mapped.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            tool_state: Some(ToolState::Completed),
            payload: NormalizedPayload::ToolCall { .. },
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

#[test]
fn file_and_diff_tools_project_protocol_locations_on_calls_and_updates() {
    let cases = [
        ("read", "Read file", ToolCategory::Read),
        ("other", "Apply diff", ToolCategory::Edit),
    ];
    for (index, (kind, title, expected)) in cases.into_iter().enumerate() {
        let mut adapter = translator(170 + index as u64);
        initialize(&mut adapter, false);
        new_session(&mut adapter);
        begin_prompt(&mut adapter);
        let tool_id = format!("located-{index}");
        let start = serde_json::to_vec(&json!({
            "jsonrpc":"2.0", "method":"session/update", "params":{
                "sessionId":"omp-session-1", "update":{
                    "sessionUpdate":"tool_call", "toolCallId":tool_id, "kind":kind,
                    "title":title, "status":"pending",
                    "locations":[{"path":"C:\\fixture\\src\\lib.rs"}]
                }
            }
        }))
        .unwrap();
        let events = adapter.ingest(&start, &FixtureResources).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            AdapterEvent::Item {
                payload: NormalizedPayload::ToolCall { category, locations, .. },
                ..
            } if *category == expected
                && matches!(
                    locations.as_slice(),
                    [SafeResourceLocation::Resource { resource }]
                        if resource.relative_path.as_str() == "src/lib.rs"
                )
        )));

        let update = serde_json::to_vec(&json!({
            "jsonrpc":"2.0", "method":"session/update", "params":{
                "sessionId":"omp-session-1", "update":{
                    "sessionUpdate":"tool_call_update", "toolCallId":tool_id,
                    "status":"in_progress"
                }
            }
        }))
        .unwrap();
        let events = adapter.ingest(&update, &FixtureResources).unwrap();
        assert!(matches!(
            events.as_slice(),
            [AdapterEvent::Item {
                payload: NormalizedPayload::ToolCall { category, locations, .. },
                ..
            }] if *category == expected
                && matches!(
                    locations.as_slice(),
                    [SafeResourceLocation::Resource { resource }]
                        if resource.relative_path.as_str() == "src/lib.rs"
                )
        ));

        let cleared_locations = if index == 0 { Value::Null } else { json!([]) };
        let clear = serde_json::to_vec(&json!({
            "jsonrpc":"2.0", "method":"session/update", "params":{
                "sessionId":"omp-session-1", "update":{
                    "sessionUpdate":"tool_call_update", "toolCallId":tool_id,
                    "status":"completed", "locations":cleared_locations
                }
            }
        }))
        .unwrap();
        let events = adapter.ingest(&clear, &FixtureResources).unwrap();
        assert!(matches!(
            events.as_slice(),
            [AdapterEvent::Item {
                payload: NormalizedPayload::ToolCall { category, locations, .. },
                ..
            }] if *category == expected && locations.as_slice().is_empty()
        ));
    }
}

#[test]
fn tool_protocol_locations_are_bounded_malformed_safe_and_exclude_web() {
    fn file_call(tool_id: &str, locations: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "jsonrpc":"2.0", "method":"session/update", "params":{
                "sessionId":"omp-session-1", "update":{
                    "sessionUpdate":"tool_call", "toolCallId":tool_id, "kind":"edit",
                    "status":"pending", "locations":locations
                }
            }
        }))
        .unwrap()
    }

    let exact = Value::Array(
        (0..COMMAND_RESOURCE_LIMIT)
            .map(|_| json!({"path":"C:\\fixture\\src\\lib.rs"}))
            .collect(),
    );
    let mut exact_adapter = translator(180);
    initialize(&mut exact_adapter, false);
    new_session(&mut exact_adapter);
    begin_prompt(&mut exact_adapter);
    let exact_events = exact_adapter
        .ingest(&file_call("exact-locations", exact), &FixtureResources)
        .unwrap();
    assert!(exact_events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { locations, .. },
            ..
        } if locations.as_slice().len() == COMMAND_RESOURCE_LIMIT
    )));

    let over_cap = Value::Array(
        (0..=COMMAND_RESOURCE_LIMIT)
            .map(|_| json!({"path":"C:\\fixture\\src\\lib.rs"}))
            .collect(),
    );
    let mut over_cap_adapter = translator(181);
    initialize(&mut over_cap_adapter, false);
    new_session(&mut over_cap_adapter);
    begin_prompt(&mut over_cap_adapter);
    assert!(matches!(
        over_cap_adapter.ingest(
            &file_call("over-cap-locations", over_cap),
            &FixtureResources
        ),
        Err(TranslationError::Limit("tool locations"))
    ));

    for (epoch, malformed) in [
        (182, json!([{"url":"https://example.com/private"}])),
        (183, json!({"path":"C:\\fixture\\src\\lib.rs"})),
    ] {
        let mut adapter = translator(epoch);
        initialize(&mut adapter, false);
        new_session(&mut adapter);
        begin_prompt(&mut adapter);
        assert!(matches!(
            adapter.ingest(
                &file_call("malformed-locations", malformed),
                &FixtureResources
            ),
            Err(TranslationError::MalformedFrame)
        ));
    }

    let secret_path = r"C:\Users\private\secret.txt";
    let mut unavailable_adapter = translator(184);
    initialize(&mut unavailable_adapter, false);
    new_session(&mut unavailable_adapter);
    begin_prompt(&mut unavailable_adapter);
    let unavailable = unavailable_adapter
        .ingest(
            &file_call("unavailable-location", json!([{"path":secret_path}])),
            &RejectResources,
        )
        .unwrap();
    assert!(unavailable.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { locations, .. },
            ..
        } if matches!(
            locations.as_slice(),
            [SafeResourceLocation::Unavailable {
                reason: UnavailableResourceReason::Foreign,
                label,
                ..
            }] if label.as_str() == "Path is outside the active project roots"
        )
    )));
    let rendered = format!("{unavailable:?}");
    assert!(!rendered.contains(secret_path));
    assert!(!rendered.contains("Users"));
    assert!(!rendered.contains("secret.txt"));

    let mut web_adapter = translator(185);
    initialize(&mut web_adapter, false);
    new_session(&mut web_adapter);
    begin_prompt(&mut web_adapter);
    let web = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"tool_call","toolCallId":"web-location","kind":"fetch","status":"pending","locations":[{"url":"https://example.com/private"}]}}}"#;
    let web = web_adapter.ingest(web, &FixtureResources).unwrap();
    assert!(web.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall {
                category: ToolCategory::Fetch,
                locations,
                ..
            },
            ..
        } if locations.as_slice().is_empty()
    )));
}

#[test]
fn file_location_snapshots_share_the_existing_aggregate_byte_bound() {
    let long_path = format!("/{}", "x".repeat(16_000));
    let locations = Value::Array(
        (0..COMMAND_RESOURCE_LIMIT)
            .map(|_| json!({"path":long_path}))
            .collect(),
    );
    let mut adapter = translator(186);
    initialize(&mut adapter, false);
    new_session(&mut adapter);
    begin_prompt(&mut adapter);
    for index in 0..8 {
        let frame = serde_json::to_vec(&json!({
            "jsonrpc":"2.0", "method":"session/update", "params":{
                "sessionId":"omp-session-1", "update":{
                    "sessionUpdate":"tool_call", "toolCallId":format!("large-file-{index}"),
                    "kind":"read", "status":"pending", "locations":locations
                }
            }
        }))
        .unwrap();
        adapter.ingest(&frame, &LongResources).unwrap();
    }
    let plus_one = serde_json::to_vec(&json!({
        "jsonrpc":"2.0", "method":"session/update", "params":{
            "sessionId":"omp-session-1", "update":{
                "sessionUpdate":"tool_call", "toolCallId":"large-file-plus-one",
                "kind":"read", "status":"pending", "locations":locations
            }
        }
    }))
    .unwrap();
    assert!(matches!(
        adapter.ingest(&plus_one, &LongResources),
        Err(TranslationError::Limit("tool snapshot bytes"))
    ));
}

#[test]
fn command_protocol_locations_replace_snapshot_locations_and_sparse_updates_retain_them() {
    let mut adapter = translator(186);
    initialize(&mut adapter, false);
    new_session(&mut adapter);
    begin_prompt(&mut adapter);
    let initial = json!({
        "jsonrpc":"2.0", "method":"session/update", "params":{
            "sessionId":"omp-session-1", "update":{
                "sessionUpdate":"tool_call", "toolCallId":"command-precedence", "kind":"execute",
                "status":"pending", "content":[{"type":"text","text":"output"}],
                "locations":[{"path":"C:\\foreign\\snapshot.rs"}]
            }
        }
    });
    let initial = adapter
        .ingest(&serde_json::to_vec(&initial).unwrap(), &RejectResources)
        .unwrap();
    assert!(initial.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { locations, .. },
            ..
        } if matches!(
            locations.as_slice(),
            [SafeResourceLocation::Unavailable {
                reason: UnavailableResourceReason::Foreign,
                ..
            }]
        )
    )));

    let protocol_update = json!({
        "jsonrpc":"2.0", "method":"session/update", "params":{
            "sessionId":"omp-session-1", "update":{
                "sessionUpdate":"tool_call_update", "toolCallId":"command-precedence",
                "status":"in_progress",
                "locations":[{"path":"C:\\fixture\\src\\lib.rs"}]
            }
        }
    });
    let protocol_update = adapter
        .ingest(
            &serde_json::to_vec(&protocol_update).unwrap(),
            &FixtureResources,
        )
        .unwrap();
    assert!(matches!(
        protocol_update.as_slice(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { locations, .. },
            ..
        }] if matches!(
            locations.as_slice(),
            [SafeResourceLocation::Resource { resource }]
                if resource.relative_path.as_str() == "src/lib.rs"
        )
    ));

    let sparse_update = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"command-precedence","status":"completed"}}}"#;
    let sparse_update = adapter.ingest(sparse_update, &RejectResources).unwrap();
    assert!(matches!(
        sparse_update.as_slice(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { locations, .. },
            ..
        }] if matches!(
            locations.as_slice(),
            [SafeResourceLocation::Resource { resource }]
                if resource.relative_path.as_str() == "src/lib.rs"
        )
    ));
}

#[test]
fn command_tool_initial_and_sparse_updates_use_the_tracked_command_category() {
    let mut adapter = translator(17);
    initialize(&mut adapter, false);
    new_session(&mut adapter);
    begin_prompt(&mut adapter);
    let initial = json!({
        "jsonrpc":"2.0", "method":"session/update", "params":{
            "sessionId":"omp-session-1", "update":{
                "sessionUpdate":"tool_call", "toolCallId":"command-1", "kind":"execute",
                "status":"pending", "content":[
                    {"type":"text","text":"\"Bearer\","},
                    {"type":"text","text":"  \t\r\n"},
                    {"type":"text","text":"opaque-command-credential path=C:"},
                    {"type":"text","text":"\\private file:///secret"}
                ],
                "locations":[{"path":"C:\\fixture\\src\\lib.rs"}]
            }
        }
    });
    let initial = adapter
        .ingest(&serde_json::to_vec(&initial).unwrap(), &FixtureResources)
        .unwrap();
    assert!(initial.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            tool_state: Some(ToolState::Pending),
            payload: NormalizedPayload::ToolCall {
                category: ToolCategory::Execute,
                output_summary: Some(output),
                locations,
                ..
            },
            ..
        } if !output.as_str().contains("opaque-command-credential")
            && !output.as_str().contains("private")
            && !output.as_str().contains("file:///")
            && locations.as_slice().len() == 1
    )));
    assert!(
        initial
            .iter()
            .any(|event| matches!(event, AdapterEvent::ToolAuthorityDenied { .. }))
    );
    assert!(
        initial
            .iter()
            .any(|event| matches!(event, AdapterEvent::CancelDispatchRequired))
    );

    let update = json!({
        "jsonrpc":"2.0", "method":"session/update", "params":{
            "sessionId":"omp-session-1", "update":{
                "sessionUpdate":"tool_call_update", "toolCallId":"command-1", "status":"in_progress"
            }
        }
    });
    let update = adapter
        .ingest(&serde_json::to_vec(&update).unwrap(), &FixtureResources)
        .unwrap();
    assert!(matches!(
        update.as_slice(),
        [AdapterEvent::Item {
            tool_state: Some(ToolState::InProgress),
            payload: NormalizedPayload::ToolCall {
                category: ToolCategory::Execute,
                output_summary: Some(output),
                locations,
                ..
            },
            ..
        }] if !output.as_str().contains("opaque-command-credential")
            && !output.as_str().contains("private")
            && !output.as_str().contains("file:///")
            && locations.as_slice().len() == 1
    ));

    let patched = json!({
        "jsonrpc":"2.0", "method":"session/update", "params":{
            "sessionId":"omp-session-1", "update":{
                "sessionUpdate":"tool_call_update", "toolCallId":"command-1", "status":"in_progress",
                "content":[
                    {"type":"text","text":"\"Bearer:\","},
                    {"type":"text","text":"\t\r\n"},
                    {"type":"text","text":"opaque-command-credential path=C:"},
                    {"type":"text","text":"\\private file:///update-secret"}
                ]
            }
        }
    });
    let patched = adapter
        .ingest(&serde_json::to_vec(&patched).unwrap(), &FixtureResources)
        .unwrap();
    assert!(matches!(
        patched.as_slice(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall {
                category: ToolCategory::Execute,
                output_summary: Some(output),
                locations,
                ..
            },
            ..
        }] if !output.as_str().contains("opaque-command-credential")
            && !output.as_str().contains("private")
            && !output.as_str().contains("file:///")
            && locations.as_slice().len() == 1
    ));

    let invalid = json!({
        "jsonrpc":"2.0", "method":"session/update", "params":{
            "sessionId":"omp-session-1", "update":{
                "sessionUpdate":"tool_call_update", "toolCallId":"command-1", "content":{}
            }
        }
    });
    assert!(matches!(
        adapter.ingest(&serde_json::to_vec(&invalid).unwrap(), &FixtureResources),
        Err(TranslationError::MalformedFrame)
    ));
    let clear = json!({
        "jsonrpc":"2.0", "method":"session/update", "params":{
            "sessionId":"omp-session-1", "update":{
                "sessionUpdate":"tool_call_update", "toolCallId":"command-1", "status":"completed",
                "content":[], "locations":[]
            }
        }
    });
    let clear = adapter
        .ingest(&serde_json::to_vec(&clear).unwrap(), &FixtureResources)
        .unwrap();
    assert!(matches!(
        clear.as_slice(),
        [AdapterEvent::Item {
            tool_state: Some(ToolState::Completed),
            payload: NormalizedPayload::ToolCall {
                category: ToolCategory::Execute,
                output_summary: None,
                locations,
                ..
            },
            ..
        }] if locations.as_slice().is_empty()
    ));
}
