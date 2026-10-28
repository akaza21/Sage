use std::{
    fs, io,
    path::PathBuf,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::AsyncWrite;

use serde_json::{Value, json};
use sha2::Digest;
use uuid::Uuid;

use super::admission::{ClaudeAdmissionUnavailable, ClaudeBindingRegistration};
use super::*;
use crate::acp::{
    contract::{
        ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageRootId,
    },
    intent::{
        IMAGE_BASE64_MAX, ImageMediaType, NormalizedPayload, SafeResourceRef, ToolCategory,
        UnavailableResourceReason,
    },
    lifecycle::{ItemLifecycle, ToolState, TurnState},
    limits::LIMITS,
    runtime::read_text_file::{DeferredReadTextOwnerResolver, ExpectedOwnerScope},
    service::{
        DiskAllowedState, ReadTextClassification, ReadTextFileService, ReadTextFlushAck,
        ReadTextFlushState, ReadTextOwner,
    },
    transport::{DeadlineAsyncWriter, FrameReader},
};
use crate::harness_discovery::{ClaudeLaunchDiscovery, DiscoveredClaudeLaunch};

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
        (session == "claude-session-1" && expected.owner == self.0).then(|| self.0.clone())
    }
}

fn p02_frame(value: Value) -> crate::acp::transport::Frame {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(FrameReader::new(bytes.as_slice()).read_frame())
        .unwrap()
        .unwrap()
}

fn open_p02_claude_session(runtime: &mut super::runtime::ClaudeRuntime) {
    use crate::acp::bindings::adapter::{AdapterCommand, EgressResolution, HarnessAdapter};
    let initialize = runtime.command(AdapterCommand::Initialize).unwrap();
    let initialize_id =
        serde_json::from_slice::<Value>(initialize.frame().payload()).unwrap()["id"]
            .as_u64()
            .unwrap();
    runtime
        .resolve_egress(initialize.into_token(), EgressResolution::Queued)
        .unwrap();
    runtime
        .ingest(
            &p02_frame(json!({
                "jsonrpc":"2.0", "id":initialize_id, "result": {
                    "protocolVersion":1, "agentCapabilities":{},
                    "models":[{"id":"claude-reviewed","name":"Claude"}]
                }
            })),
            &FixtureResources,
        )
        .unwrap();
    let session = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: None,
        })
        .unwrap();
    let session_id = serde_json::from_slice::<Value>(session.frame().payload()).unwrap()["id"]
        .as_u64()
        .unwrap();
    runtime
        .resolve_egress(session.into_token(), EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(
            &p02_frame(json!({
                "jsonrpc":"2.0", "id":session_id,
                "result":{"sessionId":"claude-session-1"}
            })),
            &FixtureResources,
        )
        .unwrap();
}

#[test]
fn p02_claude_real_ingress_preflights_routes_service_and_preserves_string_id() {
    let root = std::env::temp_dir().join(format!("sage-p02-claude-{}", Uuid::new_v4()));
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
        session_id: SageAgentSessionId::parse("session-claude-runtime").unwrap(),
        runtime_epoch: RuntimeEpoch::new(3).unwrap(),
    };
    let service = ReadTextFileService::new(database);
    let expected = ExpectedOwnerScope {
        binding_digest: [7; 32],
        owner: owner.clone(),
    };
    let path = serde_json::to_string(&file.display().to_string()).unwrap();
    let request = format!(
        r#"{{"jsonrpc":"2.0","id":"claude-7","method":"fs/read_text_file","params":{{"sessionId":"claude-session-1","path":{},"line":2,"limit":1,"_meta":{{"opaque":true}}}}}}"#,
        path
    );
    let mut bytes = request.as_bytes().to_vec();
    bytes.push(b'\n');
    let frame = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(FrameReader::new(bytes.as_slice()).read_frame())
        .unwrap()
        .unwrap();
    let mut runtime = super::runtime::ClaudeRuntime::fixture();
    open_p02_claude_session(&mut runtime);
    let handoff = runtime
        .ingest_deferred_read_text_file(&frame, &expected, &P02Owner(owner.clone()), &service)
        .unwrap();
    for state in [
        ReadTextFlushState::Classify {
            state: ReadTextClassification::NotOpen,
            editor_revision: None,
        },
        ReadTextFlushState::DiskAllowed {
            state: DiskAllowedState::NotOpen,
            editor_revision: None,
        },
    ] {
        service
            .acknowledge(ReadTextFlushAck {
                owner: handoff.begin.event.owner.clone(),
                resource: handoff.begin.event.resource.clone(),
                expected_editor_revision: handoff.begin.event.expected_editor_revision,
                token: handoff.begin.event.token.clone().into(),
                state,
            })
            .unwrap();
    }
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
        json!({"jsonrpc":"2.0","id":"claude-7","result":{"content":"two\n"}})
    );
    for malformed in [
        request.replace("\"id\":\"claude-7\"", "\"id\":1.5"),
        request.replace("\"id\":\"claude-7\"", "\"id\":9223372036854775808"),
        request.replace("\"_meta\":{\"opaque\":true}", "\"_meta\":true"),
        request.replace("\"claude-session-1\"", "\"foreign-session\""),
    ] {
        let frame = p02_frame(serde_json::from_str(&malformed).unwrap());
        assert!(
            runtime
                .ingest_deferred_read_text_file(
                    &frame,
                    &expected,
                    &P02Owner(owner.clone()),
                    &service
                )
                .is_err()
        );
    }
    // Every immutable runtime and owner-scope field is independently bound.
    let frame = p02_frame(serde_json::from_str(&request).unwrap());
    let mut wrong_binding = expected.clone();
    wrong_binding.binding_digest = [8; 32];
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
                .ingest_deferred_read_text_file(&frame, &scope, &P02Owner(owner.clone()), &service)
                .is_err()
        );
    }
    fs::remove_dir_all(root).unwrap();
}

const PNG_1X1: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

struct FixtureResources;

impl crate::acp::bindings::adapter::ResourceMapper for FixtureResources {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        self.map_absolute(absolute_path)
    }
}

impl ResourceMapper for FixtureResources {
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        if absolute_path == r"C:\fixture\root\src\lib.rs" {
            SafeResourceRef::new(
                SageRootId::parse("root-fixture")
                    .map_err(|_| UnavailableResourceReason::Unsupported)?,
                "src/lib.rs",
            )
            .map_err(|_| UnavailableResourceReason::Unsupported)
        } else {
            Err(UnavailableResourceReason::Foreign)
        }
    }
}

fn fixture(name: &str) -> Value {
    serde_json::from_slice(fixture_bytes(name)).unwrap()
}

fn fixture_bytes(name: &str) -> &'static [u8] {
    match name {
        "initialize" => include_str!("../../../../testdata/acp/claude/initialize.json"),
        "tool" => include_str!("../../../../testdata/acp/claude/tool-lifecycle.json"),
        "permission" => include_str!("../../../../testdata/acp/claude/permission.json"),
        "unsupported" => include_str!("../../../../testdata/acp/claude/unsupported.json"),
        "terminal" => include_str!("../../../../testdata/acp/claude/prompt-terminal.json"),
        _ => panic!("unknown fixture"),
    }
    .as_bytes()
}

fn encoded(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn past_turn_start(events: &[AdapterEvent]) -> &[AdapterEvent] {
    events
        .strip_prefix(&[AdapterEvent::TurnRunning])
        .unwrap_or(events)
}

#[test]
fn capabilities_intersect_with_the_conservative_h3_ceiling() {
    let offer = ClaudeCapabilityOffer::from_initialize_bytes(fixture_bytes("initialize")).unwrap();
    assert_eq!(offer.models.len(), 1);
    assert_eq!(offer.models[0].id.as_str(), "claude-reviewed");
    assert_eq!(offer.models[0].label.as_str(), "Claude reviewed fixture");
    let negotiated = negotiate_capabilities(&offer).unwrap();
    assert!(negotiated.new_session);
    assert!(negotiated.load_session);
    assert!(negotiated.prompt);
    assert!(negotiated.cancel);
    assert!(!negotiated.resume_session);
    assert!(!negotiated.close_session);
    assert!(!negotiated.native_session_fork);
    assert!(negotiated.multi_root_extension_schema_id.is_none());
    assert_eq!(negotiated.model_count, 1);
}

#[test]
fn capability_metadata_cannot_escalate_protocol_authority() {
    let mut value = fixture("initialize");
    value["result"]["protocolVersion"] = json!("2-draft");
    let offer = ClaudeCapabilityOffer::from_initialize_bytes(&encoded(&value)).unwrap();
    assert!(negotiate_capabilities(&offer).is_err());
}

#[test]
fn model_metadata_redacts_labels_and_rejects_unsafe_ids() {
    let mut value = fixture("initialize");
    value["result"]["models"][0]["name"] = json!(r"Claude C:\private\model sk-ant-secret");
    let offer = ClaudeCapabilityOffer::from_initialize_bytes(&encoded(&value)).unwrap();
    assert_eq!(
        offer.models[0].label.as_str(),
        "Claude [redacted] [redacted]"
    );
    value["result"]["models"][0]["id"] = json!(r"C:\private\model");
    assert!(ClaudeCapabilityOffer::from_initialize_bytes(&encoded(&value)).is_err());
}

#[test]
fn raw_decode_rejects_malformed_duplicate_deep_and_limit_plus_one_shapes() {
    assert!(super::wire::decode_frame(b"{}{}").is_err());
    assert!(super::wire::decode_frame(br#"{"a":1,"\u0061":2}"#).is_err());
    let deep = format!("{}0{}", "{\"x\":".repeat(33), "}".repeat(33));
    assert!(super::wire::decode_frame(deep.as_bytes()).is_err());
    let array = json!({ "items": vec![0_u8; 513] });
    assert!(super::wire::decode_frame(&encoded(&array)).is_err());
    let object = (0..257)
        .map(|index| format!("\"k{index}\":0"))
        .collect::<Vec<_>>()
        .join(",");
    assert!(super::wire::decode_frame(format!("{{{object}}}").as_bytes()).is_err());
}

#[test]
fn message_and_plan_text_redact_native_paths_and_secrets() {
    let message = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": r"Result C:\private\file sk-ant-fixture"
                }
            }
        }
    });
    let plan = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "plan",
                "entries": [{
                    "content": "/private/file token=secret",
                    "status": "pending"
                }]
            }
        }
    });
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    for frame in [message, plan] {
        let events = translator
            .translate_notification(&encoded(&frame), &FixtureResources)
            .unwrap();
        let projected = format!("{events:?}");
        assert!(projected.contains("[redacted]"));
        assert!(!projected.contains("private"));
        assert!(!projected.contains("sk-ant"));
        assert!(!projected.contains("token=secret"));
    }
}

fn image_chunk(content: Value) -> Value {
    json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "itemId": "image-one",
                "content": content
            }
        }
    })
}

#[test]
fn agent_image_chunk_admits_png_mime_and_base64() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    let events = translator
        .translate_notification(
            &encoded(&image_chunk(json!({
                "type": "image",
                "mimeType": "image/png",
                "data": PNG_1X1
            }))),
            &FixtureResources,
        )
        .unwrap();
    let [
        AdapterEvent::Item {
            lifecycle: ItemLifecycle::Completed,
            payload: NormalizedPayload::Image {
                media_type, data, ..
            },
            ..
        },
    ] = past_turn_start(&events)
    else {
        panic!("expected one admitted image, got {events:?}")
    };
    assert_eq!(*media_type, ImageMediaType::Png);
    assert_eq!(data.as_str(), PNG_1X1);
}

#[test]
fn agent_image_chunk_rejects_svg_uri_oversize_and_missing_bytes() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    let cases = [
        (
            json!({
                "type": "image",
                "mimeType": "image/svg+xml",
                "data": "PHN2Zy8+"
            }),
            "Image media type is not admitted",
        ),
        (
            json!({
                "type": "image",
                "mimeType": "image/png",
                "data": PNG_1X1,
                "uri": "https://example.com/x.png"
            }),
            "Image media type is not admitted",
        ),
        (
            json!({
                "type": "image",
                "mimeType": "image/png",
                "data": "A".repeat(IMAGE_BASE64_MAX)
            }),
            "Image exceeds the bounded size cap",
        ),
        (
            json!({
                "type": "image",
                "mimeType": "image/png"
            }),
            "Image payload is missing",
        ),
    ];
    for (content, summary) in cases {
        let events = translator
            .translate_notification(&encoded(&image_chunk(content)), &FixtureResources)
            .unwrap();
        assert!(
            matches!(
                past_turn_start(&events),
                [AdapterEvent::Item {
                    lifecycle: ItemLifecycle::Unsupported,
                    payload: NormalizedPayload::Unsupported { summary: got, .. },
                    ..
                }] if got.as_str() == summary
            ),
            "expected {summary}, got {events:?}"
        );
    }
}

#[test]
fn agent_audio_and_resource_blocks_stay_unsupported() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    for content in [
        json!({ "type": "audio", "mimeType": "audio/wav", "data": "AAAA" }),
        json!({ "type": "resource", "resource": { "uri": "file:///tmp/x" } }),
    ] {
        let events = translator
            .translate_notification(&encoded(&image_chunk(content)), &FixtureResources)
            .unwrap();
        assert!(matches!(
            past_turn_start(&events),
            [AdapterEvent::Item {
                lifecycle: ItemLifecycle::Unsupported,
                payload: NormalizedPayload::Unsupported { summary, .. },
                ..
            }] if summary.as_str() == "Non-text content is not admitted"
        ));
    }
}

#[test]
fn message_chunks_emit_complete_bounded_snapshots_and_reset_between_turns() {
    fn chunk(text: &str) -> Value {
        json!({
            "method": "session/update",
            "params": {
                "sessionId": "claude-session-1",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "itemId": "message-one",
                    "content": { "type": "text", "text": text }
                }
            }
        })
    }

    fn text(events: &[AdapterEvent]) -> &str {
        let [
            AdapterEvent::Item {
                payload: NormalizedPayload::Text { text },
                ..
            },
        ] = past_turn_start(events)
        else {
            panic!("expected one text snapshot")
        };
        text.as_str()
    }

    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    let first = translator
        .translate_notification(&encoded(&chunk("Hel")), &FixtureResources)
        .unwrap();
    assert_eq!(text(&first), "Hel");
    let second = translator
        .translate_notification(&encoded(&chunk("lo")), &FixtureResources)
        .unwrap();
    assert_eq!(text(&second), "Hello");
    translator
        .translate_prompt_result(&encoded(
            &json!({"id":"prompt-1","result":{"stopReason":"end_turn"}}),
        ))
        .unwrap();

    translator.begin_prompt("prompt-2").unwrap();
    let next_turn = translator
        .translate_notification(&encoded(&chunk("Again")), &FixtureResources)
        .unwrap();
    assert_eq!(text(&next_turn), "Again");
}

#[test]
fn message_snapshot_redacts_tokens_split_across_frame_boundaries() {
    let chunk = |text: &str| {
        json!({
            "method": "session/update",
            "params": {
                "sessionId": "claude-session-1",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "itemId": "message-one",
                    "content": { "type": "text", "text": text }
                }
            }
        })
    };
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    translator
        .translate_notification(&encoded(&chunk("Bearer ")), &FixtureResources)
        .unwrap();
    translator
        .translate_notification(&encoded(&chunk("secret C:")), &FixtureResources)
        .unwrap();
    let events = translator
        .translate_notification(&encoded(&chunk(r"\private\file.txt")), &FixtureResources)
        .unwrap();
    let projected = format!("{events:?}");
    assert!(projected.contains("[redacted]"));
    assert!(!projected.contains("secret"));
    assert!(!projected.contains("private"));
}

#[test]
fn accumulated_message_limit_rejects_before_mutating_the_snapshot() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    let first = "a".repeat(LIMITS.wire.normalized_item_bytes as usize - 1);
    let frame = |text: &str| {
        json!({
            "method": "session/update",
            "params": {
                "sessionId": "claude-session-1",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "itemId": "message-one",
                    "content": { "type": "text", "text": text }
                }
            }
        })
    };
    translator
        .translate_notification(&encoded(&frame(&first)), &FixtureResources)
        .unwrap();
    assert!(matches!(
        translator.translate_notification(&encoded(&frame("xx")), &FixtureResources),
        Err(TranslationError::ItemLimit)
    ));
    let exact = translator
        .translate_notification(&encoded(&frame("b")), &FixtureResources)
        .unwrap();
    let [
        AdapterEvent::Item {
            payload: NormalizedPayload::Text { text },
            ..
        },
    ] = exact.as_slice()
    else {
        panic!("expected exact-limit text snapshot")
    };
    assert_eq!(
        text.as_str().len(),
        LIMITS.wire.normalized_item_bytes as usize
    );
    assert!(text.as_str().ends_with('b'));
}

#[test]
fn prompt_completion_requires_the_exact_pending_request() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-expected").unwrap();
    let foreign = json!({ "id": "prompt-foreign", "result": { "stopReason": "end_turn" } });
    assert!(matches!(
        translator.translate_prompt_result(&encoded(&foreign)),
        Err(TranslationError::ForeignId)
    ));
    let expected = json!({ "id": "prompt-expected", "result": { "stopReason": "end_turn" } });
    assert!(matches!(
        past_turn_start(
            &translator
                .translate_prompt_result(&encoded(&expected))
                .unwrap()
        ),
        [AdapterEvent::TurnTerminal(PromptTerminal::Completed)]
    ));
}

#[test]
fn one_session_runs_two_ordered_turns_and_retains_old_terminal_idempotency() {
    let first = json!({ "id": "prompt-1", "result": { "stopReason": "end_turn" } });
    let second = json!({ "id": "prompt-2", "result": { "stopReason": "cancelled" } });
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    assert!(matches!(
        translator
            .translate_prompt_result(&encoded(&first))
            .unwrap()
            .as_slice(),
        [
            AdapterEvent::TurnRunning,
            AdapterEvent::TurnTerminal(PromptTerminal::Completed)
        ]
    ));
    translator.begin_prompt("prompt-2").unwrap();
    assert!(matches!(
        translator
            .translate_prompt_result(&encoded(&first))
            .unwrap()
            .as_slice(),
        [AdapterEvent::IgnoredDuplicate]
    ));
    assert!(matches!(
        translator
            .translate_prompt_result(&encoded(&second))
            .unwrap()
            .as_slice(),
        [
            AdapterEvent::TurnRunning,
            AdapterEvent::TurnTerminal(PromptTerminal::Cancelled)
        ]
    ));
}

#[test]
fn correlated_rpc_error_is_redacted_and_terminalized() {
    let error = json!({
        "id": "prompt-error",
        "error": {
            "code": -32000,
            "message": r"failed at C:\private\file with sk-ant-fixture"
        }
    });
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-error").unwrap();
    let events = translator
        .translate_prompt_result(&encoded(&error))
        .unwrap();
    assert!(matches!(
        past_turn_start(&events),
        [
            AdapterEvent::Item {
                lifecycle: ItemLifecycle::Failed,
                payload: NormalizedPayload::Error { .. },
                ..
            },
            AdapterEvent::TurnTerminal(PromptTerminal::Failed)
        ]
    ));
    let projected = format!("{events:?}");
    assert!(projected.contains("[redacted]"));
    assert!(!projected.contains("private"));
    assert!(!projected.contains("sk-ant"));
}

#[test]
fn tool_and_diff_normalization_excludes_raw_commands_secrets_and_paths() {
    let frames = fixture("tool").as_array().unwrap().clone();
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    let start = translator
        .translate_notification(&encoded(&frames[0]), &FixtureResources)
        .unwrap();
    match &start[0] {
        AdapterEvent::Item {
            lifecycle: ItemLifecycle::Started,
            tool_state: Some(ToolState::Pending),
            payload:
                NormalizedPayload::ToolCall {
                    category: ToolCategory::Edit,
                    input_summary: None,
                    locations,
                    ..
                },
            ..
        } => assert!(matches!(
            locations.as_slice(),
            [crate::acp::intent::SafeResourceLocation::Resource { resource }]
                if resource.relative_path.as_str() == "src/lib.rs"
        )),
        other => panic!("unexpected event: {other:?}"),
    }

    let completed = translator
        .translate_notification(&encoded(&frames[1]), &FixtureResources)
        .unwrap();
    assert_eq!(completed.len(), 3);
    match &completed[0] {
        AdapterEvent::Item {
            lifecycle: ItemLifecycle::Completed,
            tool_state: Some(ToolState::Completed),
            payload:
                NormalizedPayload::ToolCall {
                    output_summary: None,
                    locations,
                    ..
                },
            ..
        } => assert!(matches!(
            locations.as_slice(),
            [crate::acp::intent::SafeResourceLocation::Resource { resource }]
                if resource.relative_path.as_str() == "src/lib.rs"
        )),
        other => panic!("unexpected event: {other:?}"),
    }
    match &completed[1] {
        AdapterEvent::Item {
            payload:
                NormalizedPayload::FileChangeDiff {
                    resource,
                    additions,
                    deletions,
                    ..
                },
            ..
        } => {
            let crate::acp::intent::SafeResourceLocation::Resource { resource } = resource else {
                panic!("expected mapped resource");
            };
            assert_eq!(resource.relative_path.as_str(), "src/lib.rs");
            assert_eq!(*additions, Some(2));
            assert_eq!(*deletions, Some(1));
        }
        other => panic!("unexpected event: {other:?}"),
    }
    match &completed[2] {
        AdapterEvent::FilePair {
            old_text, new_text, ..
        } => {
            assert!(!old_text.is_empty() || !new_text.is_empty());
        }
        other => panic!("expected admitted file pair: {other:?}"),
    }
    let projected = format!("{start:?}{completed:?}");
    assert!(!projected.contains("secret-command"));
    assert!(!projected.contains("sk-ant-fixture"));
    assert!(!projected.contains(r"C:\fixture"));
}

#[test]
fn command_tool_initial_and_sparse_updates_retain_only_typed_output_and_locations() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("command-prompt").unwrap();
    let initial = json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call", "toolCallId":"command-1", "kind":"execute",
            "status":"pending", "content":[
                {"type":"text","text":"\"Bearer\","},
                {"type":"text","text":"  \t\r\n"},
                {"type":"text","text":"opaque-command-credential path=C:"},
                {"type":"text","text":"\\private file:///secret"}
            ],
            "locations":[{"path":"C:\\fixture\\root\\src\\lib.rs"}]
        }}
    });
    let started = translator
        .translate_notification(&encoded(&initial), &FixtureResources)
        .unwrap();
    assert!(matches!(
        past_turn_start(&started),
        [AdapterEvent::Item {
            tool_state: Some(ToolState::Pending),
            payload: NormalizedPayload::ToolCall {
                category: ToolCategory::Execute,
                input_summary: None,
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

    let status_only = json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"command-1", "status":"in_progress"
        }}
    });
    let status = translator
        .translate_notification(&encoded(&status_only), &FixtureResources)
        .unwrap();
    assert!(matches!(
        status.as_slice(),
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
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"command-1", "status":"in_progress",
            "content":[
                {"type":"text","text":"\"Bearer:\","},
                {"type":"text","text":"\t\r\n"},
                {"type":"text","text":"opaque-command-credential path=C:"},
                {"type":"text","text":"\\private file:///update-secret"}
            ]
        }}
    });
    let patched = translator
        .translate_notification(&encoded(&patched), &FixtureResources)
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
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"command-1", "content":{}
        }}
    });
    assert!(matches!(
        translator.translate_notification(&encoded(&invalid), &FixtureResources),
        Err(TranslationError::MalformedFrame)
    ));
    let cleared = json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"command-1", "status":"completed",
            "content":[], "locations":[]
        }}
    });
    let cleared = translator
        .translate_notification(&encoded(&cleared), &FixtureResources)
        .unwrap();
    assert!(matches!(
        cleared.as_slice(),
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

#[test]
fn read_tool_sparse_completion_update_retains_locations() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("read-prompt").unwrap();
    let initial = json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call", "toolCallId":"read-1", "kind":"read",
            "status":"pending", "title":"Read lib.rs", "content":[],
            "locations":[{"path": r"C:\fixture\root\src\lib.rs", "line": 1}]
        }}
    });
    let started = translator
        .translate_notification(&encoded(&initial), &FixtureResources)
        .unwrap();
    assert!(matches!(
        past_turn_start(&started),
        [AdapterEvent::Item {
            tool_state: Some(ToolState::Pending),
            payload: NormalizedPayload::ToolCall {
                category: ToolCategory::Read,
                locations,
                ..
            },
            ..
        }] if locations.as_slice().len() == 1
    ));

    let completed = json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"read-1", "status":"completed",
            "content":[{"type":"content","content":{"type":"text","text":"1\tread-only harness probe"}}]
        }}
    });
    let completed = translator
        .translate_notification(&encoded(&completed), &FixtureResources)
        .unwrap();
    let [
        AdapterEvent::Item {
            tool_state: Some(ToolState::Completed),
            payload:
                NormalizedPayload::ToolCall {
                    category: ToolCategory::Read,
                    label,
                    locations,
                    ..
                },
            ..
        },
    ] = completed.as_slice()
    else {
        panic!("unexpected events: {completed:?}");
    };
    assert_eq!(label.as_str(), "Read lib.rs");
    let [crate::acp::intent::SafeResourceLocation::Resource { resource }] = locations.as_slice()
    else {
        panic!("expected one retained resource: {locations:?}");
    };
    assert_eq!(resource.relative_path.as_str(), "src/lib.rs");

    let cleared = json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"read-1", "status":"completed",
            "locations":[]
        }}
    });
    let cleared = translator
        .translate_notification(&encoded(&cleared), &FixtureResources)
        .unwrap();
    assert!(matches!(cleared.as_slice(), [AdapterEvent::IgnoredLate]));

    let reseeded = json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"read-1", "status":"completed",
            "locations":[{"path": r"C:\fixture\root\src\lib.rs", "line": 1}]
        }}
    });
    let reseeded = translator
        .translate_notification(&encoded(&reseeded), &FixtureResources)
        .unwrap();
    assert!(matches!(reseeded.as_slice(), [AdapterEvent::IgnoredLate]));

    let nulled = json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"read-1", "status":"completed",
            "locations": null
        }}
    });
    let nulled = translator
        .translate_notification(&encoded(&nulled), &FixtureResources)
        .unwrap();
    assert!(matches!(nulled.as_slice(), [AdapterEvent::IgnoredLate]));
}

fn tool_output_summary(events: &[AdapterEvent]) -> Option<String> {
    let [
        AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { output_summary, .. },
            ..
        },
    ] = events
    else {
        panic!("unexpected events: {events:?}");
    };
    output_summary
        .as_ref()
        .map(|value| value.as_str().to_owned())
}

fn read_tool_probe(text: &str) -> Value {
    json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"read-text", "status":"completed",
            "content":[{"type":"content","content":{"type":"text","text":text}}]
        }}
    })
}

const READ_FENCE: &str = "\u{60}\u{60}\u{60}";

fn fenced_read_result(numbered: &[&str]) -> String {
    format!("{READ_FENCE}\n{}\n{READ_FENCE}", numbered.join("\n"))
}

fn read_tool_probe_start() -> Value {
    json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call", "toolCallId":"read-text", "kind":"read",
            "status":"pending", "title":"Read alpha.txt",
            "locations":[{"path": r"C:\fixture\root\src\lib.rs", "line": 1}]
        }}
    })
}

#[test]
fn read_tool_content_text_projects_retained_tool_output() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("read-prompt").unwrap();
    let started = translator
        .translate_notification(&encoded(&read_tool_probe_start()), &FixtureResources)
        .unwrap();
    assert_eq!(tool_output_summary(past_turn_start(&started)), None);

    let result = fenced_read_result(&["1\tread-only harness probe"]);
    let completed = translator
        .translate_notification(&encoded(&read_tool_probe(&result)), &FixtureResources)
        .unwrap();
    assert_eq!(tool_output_summary(&completed).as_deref(), Some(&*result));

    let retitled = json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"read-text", "status":"completed",
            "title":"Read alpha.txt (1 line)"
        }}
    });
    let retitled = translator
        .translate_notification(&encoded(&retitled), &FixtureResources)
        .unwrap();
    assert!(matches!(retitled.as_slice(), [AdapterEvent::IgnoredLate]));

    let cleared = json!({
        "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
            "sessionUpdate":"tool_call_update", "toolCallId":"read-text", "status":"completed",
            "content":[]
        }}
    });
    let cleared = translator
        .translate_notification(&encoded(&cleared), &FixtureResources)
        .unwrap();
    assert!(matches!(cleared.as_slice(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn read_tool_source_lines_survive_the_real_fenced_adapter_shape() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("read-prompt").unwrap();
    translator
        .translate_notification(&encoded(&read_tool_probe_start()), &FixtureResources)
        .unwrap();
    let result = fenced_read_result(&[
        "1\t/// Doc comment for the probe.",
        "2\tpub fn probe() {",
        "3\t    // line comment",
        "4\t    /* block comment */",
        "5\t    render(\"</div>\");",
        "6\t}",
    ]);
    let completed = translator
        .translate_notification(&encoded(&read_tool_probe(&result)), &FixtureResources)
        .unwrap();
    assert_eq!(tool_output_summary(&completed).as_deref(), Some(&*result));
}

#[test]
fn read_tool_oversize_content_text_truncates_instead_of_failing() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("read-prompt").unwrap();
    translator
        .translate_notification(&encoded(&read_tool_probe_start()), &FixtureResources)
        .unwrap();
    let completed = translator
        .translate_notification(
            &encoded(&read_tool_probe(&"probe line\n".repeat(600))),
            &FixtureResources,
        )
        .unwrap();
    let output = tool_output_summary(&completed).expect("retained tool output");
    assert!(output.starts_with("[output truncated] "), "{output}");
    assert_eq!(output.lines().count(), 200);
}

#[test]
fn read_tool_markup_content_text_survives_translation_verbatim() {
    fn verbatim(text: &str) -> Option<String> {
        let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
        translator.begin_prompt("read-prompt").unwrap();
        translator
            .translate_notification(&encoded(&read_tool_probe_start()), &FixtureResources)
            .unwrap();
        let completed = translator
            .translate_notification(&encoded(&read_tool_probe(text)), &FixtureResources)
            .unwrap();
        tool_output_summary(&completed)
    }

    assert_eq!(
        verbatim("<img src=x onerror=alert(1)>").as_deref(),
        Some("<img src=x onerror=alert(1)>")
    );
    assert_eq!(
        verbatim("<script>alert(1)</script>").as_deref(),
        Some("<script>alert(1)</script>")
    );
    assert_eq!(
        verbatim(r"<C:\fixture\secret.txt>").as_deref(),
        Some("[redacted]")
    );
}

#[test]
fn read_markup_and_code_survive_while_command_and_console_secrets_are_redacted() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("read-prompt").unwrap();
    translator
        .translate_notification(&encoded(&read_tool_probe_start()), &FixtureResources)
        .unwrap();
    let source = "<script>alert(1)</script>\n<div>ordinary markup</div>\nfn main() { println!(\"</div>\"); }";
    let read = translator
        .translate_notification(&encoded(&read_tool_probe(source)), &FixtureResources)
        .unwrap();
    assert_eq!(tool_output_summary(&read).as_deref(), Some(source));

    let started = translator
        .translate_notification(
            &encoded(&json!({
                "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
                    "sessionUpdate":"tool_call", "toolCallId":"command-text", "kind":"execute",
                    "status":"in_progress", "rawInput":{"command":"echo --token 'private value' --password \"another secret\""}
                }}
            })),
            &FixtureResources,
        )
        .unwrap();
    assert!(matches!(started.as_slice(), [AdapterEvent::Item {
        payload: NormalizedPayload::ToolCall { input_summary: Some(input), .. }, ..
    }] if input.as_str() == "echo --token [redacted] --password [redacted]"));
    let completed = translator
        .translate_notification(
            &encoded(&json!({
                "method":"session/update", "params":{"sessionId":"claude-session-1", "update":{
                    "sessionUpdate":"tool_call_update", "toolCallId":"command-text", "status":"completed",
                    "content":[{"type":"content","content":{"type":"text",
                        "text":"```console\nok --token 'private value' --password \"another secret\"\n```"}}]
                }}
            })),
            &FixtureResources,
        )
        .unwrap();
    assert_eq!(
        tool_output_summary(&completed).as_deref(),
        Some("ok --token [redacted] --password [redacted]")
    );
}

#[test]
fn file_tool_locations_reject_over_cap_and_malformed_entries() {
    fn update(locations: Value) -> Value {
        json!({
            "method": "session/update", "params": {"sessionId": "claude-session-1", "update": {
                "sessionUpdate": "tool_call", "toolCallId": "file-locations", "kind": "edit",
                "status": "pending", "locations": locations
            }}
        })
    }

    let over_cap = Value::Array(
        (0..=crate::acp::intent::COMMAND_RESOURCE_LIMIT)
            .map(|_| json!({"path": r"C:\fixture\root\src\lib.rs"}))
            .collect(),
    );
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    assert!(matches!(
        translator.translate_notification(&encoded(&update(over_cap)), &FixtureResources),
        Err(TranslationError::ToolLimit)
    ));

    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    assert!(matches!(
        translator.translate_notification(
            &encoded(&update(json!([{"url": "https://example.com"}]))),
            &FixtureResources,
        ),
        Err(TranslationError::MalformedFrame)
    ));
}

#[test]
fn permission_requests_only_produce_durable_decline_work() {
    let start = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "tool-1",
                "kind": "execute",
                "status": "in_progress"
            }
        }
    });
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();
    let events = translator
        .translate_permission_request(fixture_bytes("permission"))
        .unwrap();
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0],
        AdapterEvent::Item {
            tool_state: Some(ToolState::WaitingForApproval),
            payload: NormalizedPayload::ToolCall { .. },
            ..
        }
    ));
    assert!(matches!(
        &events[1],
        AdapterEvent::Item {
            tool_state: Some(ToolState::WaitingForApproval),
            payload: NormalizedPayload::PermissionRequest { .. },
            ..
        }
    ));
    assert!(matches!(
        translator
            .permission_decline_committed("permission-1")
            .unwrap(),
        AdapterEvent::Item {
            tool_state: Some(ToolState::Declined),
            ..
        }
    ));
    assert!(matches!(
        translator
            .permission_decline_committed("permission-1")
            .unwrap(),
        AdapterEvent::IgnoredDuplicate
    ));
    assert!(matches!(
        translator
            .translate_permission_request(fixture_bytes("permission"))
            .unwrap()
            .as_slice(),
        [AdapterEvent::IgnoredDuplicate]
    ));
}

#[test]
fn duplicate_and_conflicting_terminals_are_deterministic() {
    let terminal = fixture("terminal");
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    assert!(matches!(
        past_turn_start(
            &translator
                .translate_prompt_result(&encoded(&terminal))
                .unwrap()
        ),
        [AdapterEvent::TurnTerminal(PromptTerminal::Completed)]
    ));
    assert_eq!(PromptTerminal::Completed.turn_state(), TurnState::Completed);
    assert!(matches!(
        translator
            .translate_prompt_result(&encoded(&terminal))
            .unwrap()
            .as_slice(),
        [AdapterEvent::IgnoredDuplicate]
    ));
    let conflict = json!({ "id": "prompt-1", "result": { "stopReason": "cancelled" } });
    assert!(matches!(
        translator.translate_prompt_result(&encoded(&conflict)),
        Err(TranslationError::TerminalConflict)
    ));
    let late = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "late" }
            }
        }
    });
    assert!(matches!(
        translator
            .translate_notification(&encoded(&late), &FixtureResources)
            .unwrap()
            .as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
}

#[test]
fn thought_chunks_stream_and_nested_transcript_updates_are_bounded_fallbacks() {
    let frames = fixture("unsupported").as_array().unwrap().clone();
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    let thought = translator
        .translate_notification(&encoded(&frames[0]), &FixtureResources)
        .unwrap();
    assert!(matches!(
        &thought[0],
        AdapterEvent::Item {
            lifecycle: ItemLifecycle::Streaming,
            payload: NormalizedPayload::ThoughtSummary { summary },
            ..
        } if summary.as_str() == "unproven thought"
    ));
    let nested = translator
        .translate_notification(&encoded(&frames[1]), &FixtureResources)
        .unwrap();
    assert!(matches!(
        &nested[0],
        AdapterEvent::Item {
            lifecycle: ItemLifecycle::Unsupported,
            payload: NormalizedPayload::Unsupported { .. },
            ..
        }
    ));
    assert!(!format!("{nested:?}").contains("must-not-create-sage-worker"));
}

#[test]
fn session_metadata_updates_are_absorbed_instead_of_shown_as_activity() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    for update in [
        json!({
            "sessionUpdate": "available_commands_update",
            "availableCommands": [{ "name": "deep-research", "description": "fan out", "input": null }]
        }),
        json!({ "sessionUpdate": "session_info_update", "title": "First Heading Finder" }),
    ] {
        let frame = json!({
            "method": "session/update",
            "params": { "sessionId": "claude-session-1", "update": update }
        });
        assert!(matches!(
            translator
                .translate_notification(&encoded(&frame), &FixtureResources)
                .unwrap()
                .as_slice(),
            [AdapterEvent::Absorbed]
        ));
    }
}

#[test]
fn foreign_sessions_or_orphan_tools_fail_without_projection() {
    let foreign = json!({
        "method": "session/update",
        "params": {
            "sessionId": "foreign",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "ignored" }
            }
        }
    });
    let orphan = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "missing",
                "status": "completed"
            }
        }
    });
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    assert!(matches!(
        translator.translate_notification(&encoded(&foreign), &FixtureResources),
        Err(TranslationError::ForeignId)
    ));
    assert!(matches!(
        translator.translate_notification(&encoded(&orphan), &FixtureResources),
        Err(TranslationError::OrphanToolUpdate)
    ));
}

#[test]
fn launch_manifest_admits_valid_discovery_shape_and_rejects_contract_drift() {
    let adapter = absolute("claude-adapter.js");
    let candidate = CandidateManifest {
        adapter_version: "0.76.0".into(),
        acp_sdk_version: "1.4.0".into(),
        entrypoint: "dist/index.js".into(),
        node_engine: ">=22".into(),
        node_version: "22.18.0".into(),
        artifact_path: adapter.clone(),
        artifact_sha256: "9d73d1f0f121fb96cc8badb28c22d5bff02d8582eb2e40360a81c189e1b9422a".into(),
        runtime_path: absolute("node.exe"),
        runtime_sha256: "c22d1c59a1f767a1ed0178445a027f2257d318c55430fc819d48f269586822b7".into(),
        fixed_arguments: vec![adapter.to_string_lossy().into_owned()],
        environment_keys: super::LAUNCH_ENVIRONMENT_KEYS
            .iter()
            .map(|key| (*key).into())
            .collect(),
        project_root_count: 1,
    };
    // This discriminates because the validated candidate survives only the
    // exact Node/adapter contract; each mutation below changes one guard.
    assert!(validate_research_candidate(candidate.clone()).is_ok());
    let mut sdk_mismatch = candidate.clone();
    sdk_mismatch.acp_sdk_version = "1.5.0".into();
    assert_eq!(
        validate_research_candidate(sdk_mismatch)
            .expect("an unknown SDK degrades rather than refusing")
            .characterisation,
        crate::acp::bindings::characterisation::Characterisation::Uncharacterised
    );
    let mut empty_sdk = candidate.clone();
    empty_sdk.acp_sdk_version = String::new();
    assert_eq!(
        validate_research_candidate(empty_sdk),
        Err(ManifestError::IdentityMismatch),
        "an absent SDK version is unreadable evidence, not a degraded build"
    );
    let mut old_node = candidate.clone();
    old_node.node_version = "21.9.0".into();
    assert_eq!(
        validate_research_candidate(old_node),
        Err(ManifestError::NodeEngineMismatch)
    );
    let mut unbounded_environment = candidate;
    unbounded_environment.environment_keys.push("PATH".into());
    assert_eq!(
        validate_research_candidate(unbounded_environment),
        Err(ManifestError::Environment)
    );
}

#[test]
fn injectable_discovery_reports_each_admission_refusal_truthfully() {
    // Each case is a distinct discovery outcome, so this cannot pass if
    // admission collapses absent, unreadable, and incompatible evidence.
    for (discovery, expected) in [
        (
            ClaudeLaunchDiscovery::AdapterAbsent,
            ClaudeAdmissionUnavailable::AdapterAbsent,
        ),
        (
            ClaudeLaunchDiscovery::AdapterVersionUnreadable,
            ClaudeAdmissionUnavailable::AdapterVersionUnreadable,
        ),
        (
            ClaudeLaunchDiscovery::NodeAbsent,
            ClaudeAdmissionUnavailable::NodeAbsent,
        ),
        (
            ClaudeLaunchDiscovery::NodeVersionUnreadable,
            ClaudeAdmissionUnavailable::NodeVersionUnreadable,
        ),
    ] {
        let registration = ClaudeBindingRegistration::admitted_for_test(discovery);
        assert_eq!(registration.unavailable(), Some(expected));
        assert!(!registration.is_available());
    }
}

#[test]
fn startup_defers_claude_launch_admission_until_background_discovery_applies() {
    let registration = ClaudeBindingRegistration::startup();

    assert_eq!(
        registration.unavailable(),
        Some(ClaudeAdmissionUnavailable::DiscoveryPending)
    );
    assert!(!registration.is_available());

    registration.apply_discovery(ClaudeLaunchDiscovery::AdapterAbsent);

    assert_eq!(
        registration.unavailable(),
        Some(ClaudeAdmissionUnavailable::AdapterAbsent)
    );
    assert!(!registration.is_available());
}

#[test]
fn an_unexpected_acp_sdk_binds_uncharacterised_instead_of_being_refused() {
    let root = std::env::temp_dir().join(format!("sage-claude-uncharacterised-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let adapter = root.join("adapter.js");
    let node = root.join("node.exe");
    fs::write(&adapter, b"adapter").unwrap();
    fs::write(&node, b"node").unwrap();
    let candidate = DiscoveredClaudeLaunch {
        adapter_path: adapter.clone(),
        adapter_version: "0.76.0".into(),
        acp_sdk_version: "9.9.9".into(),
        node_engine: ">=22".into(),
        node_path: node.clone(),
        node_version: "22.18.0".into(),
        adapter_sha256: crate::harness_discovery::sha256_file(&adapter).unwrap(),
        node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
    };
    let registration = ClaudeBindingRegistration::admitted_for_test(
        ClaudeLaunchDiscovery::Present(Box::new(candidate)),
    );
    assert_eq!(registration.unavailable(), None);
    let binding = registration.refresh().expect("unknown SDK still binds");
    assert_eq!(
        binding.characterisation(),
        crate::acp::bindings::characterisation::Characterisation::Uncharacterised,
        "an unknown SDK must be reported, not hidden behind a known profile"
    );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_restated_but_equivalent_node_engine_range_is_not_an_identity_failure() {
    let root = std::env::temp_dir().join(format!("sage-claude-engine-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let adapter = root.join("adapter.js");
    let node = root.join("node.exe");
    fs::write(&adapter, b"adapter").unwrap();
    fs::write(&node, b"node").unwrap();
    let candidate = DiscoveredClaudeLaunch {
        adapter_path: adapter.clone(),
        adapter_version: "0.76.0".into(),
        acp_sdk_version: "1.4.0".into(),
        node_engine: ">=22.0.0".into(),
        node_path: node.clone(),
        node_version: "22.18.0".into(),
        adapter_sha256: crate::harness_discovery::sha256_file(&adapter).unwrap(),
        node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
    };
    let registration = ClaudeBindingRegistration::admitted_for_test(
        ClaudeLaunchDiscovery::Present(Box::new(candidate)),
    );
    assert_eq!(registration.unavailable(), None);
    assert_eq!(
        registration.refresh().unwrap().characterisation(),
        crate::acp::bindings::characterisation::Characterisation::Characterised
    );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn node_below_sages_floor_is_still_refused_even_if_the_adapter_would_allow_it() {
    let root = std::env::temp_dir().join(format!("sage-claude-floor-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let adapter = root.join("adapter.js");
    let node = root.join("node.exe");
    fs::write(&adapter, b"adapter").unwrap();
    fs::write(&node, b"node").unwrap();
    let candidate = DiscoveredClaudeLaunch {
        adapter_path: adapter.clone(),
        adapter_version: "0.76.0".into(),
        acp_sdk_version: "1.4.0".into(),
        node_engine: ">=18".into(),
        node_path: node.clone(),
        node_version: "20.11.0".into(),
        adapter_sha256: crate::harness_discovery::sha256_file(&adapter).unwrap(),
        node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
    };
    let registration = ClaudeBindingRegistration::admitted_for_test(
        ClaudeLaunchDiscovery::Present(Box::new(candidate)),
    );
    assert_eq!(
        registration.unavailable(),
        Some(ClaudeAdmissionUnavailable::NodeEngineMismatch)
    );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_same_length_same_mtime_runtime_replacement_is_reported_as_drift() {
    let root = std::env::temp_dir().join(format!("sage-claude-drift-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let adapter = root.join("adapter.js");
    let node = root.join("node.exe");
    fs::write(&adapter, b"adapter").unwrap();
    fs::write(&node, b"node-aaaa").unwrap();
    let candidate = DiscoveredClaudeLaunch {
        adapter_path: adapter.clone(),
        adapter_version: "0.76.0".into(),
        acp_sdk_version: "1.4.0".into(),
        node_engine: ">=22".into(),
        node_path: node.clone(),
        node_version: "22.18.0".into(),
        adapter_sha256: crate::harness_discovery::sha256_file(&adapter).unwrap(),
        node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
    };
    let registration = ClaudeBindingRegistration::admitted_for_test(
        ClaudeLaunchDiscovery::Present(Box::new(candidate)),
    );
    let binding = registration.refresh().expect("candidate is admitted");
    let modified = fs::metadata(&node).unwrap().modified().unwrap();
    let primed = crate::harness_discovery::cached_sha256_file(&node).expect("prime digest cache");
    fs::write(&node, b"node-bbbb").unwrap();
    fs::File::options()
        .write(true)
        .open(&node)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(
        crate::harness_discovery::cached_sha256_file(&node),
        Some(primed),
        "the discovery cache still reports the stale digest for this replacement"
    );
    assert_eq!(binding.revalidate(), Err(ClaudeAdmissionUnavailable::Drift));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn unix_launch_environment_defaults_the_config_directory_under_home() {
    let derived = |pairs: &[(&str, &str)]| {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        super::admission::unix_launch_environment(move |key| {
            owned
                .iter()
                .find(|(candidate, _)| candidate == key)
                .map(|(_, value)| std::ffi::OsString::from(value))
        })
    };
    let expected_default = PathBuf::from("/home/sage").join(".claude").into_os_string();
    assert_eq!(
        derived(&[("HOME", "/home/sage"), ("PATH", "/usr/bin")]),
        Ok(vec![
            ("CLAUDE_CONFIG_DIR".into(), expected_default),
            ("HOME".into(), "/home/sage".into()),
            ("PATH".into(), "/usr/bin".into()),
        ])
    );
    assert_eq!(
        derived(&[
            ("HOME", "/home/sage"),
            ("PATH", "/usr/bin"),
            ("CLAUDE_CONFIG_DIR", "/elsewhere/.claude"),
            ("TMPDIR", "/tmp/sage"),
        ]),
        Ok(vec![
            ("CLAUDE_CONFIG_DIR".into(), "/elsewhere/.claude".into()),
            ("HOME".into(), "/home/sage".into()),
            ("PATH".into(), "/usr/bin".into()),
            ("TMPDIR".into(), "/tmp/sage".into()),
        ])
    );
    assert_eq!(
        derived(&[
            ("HOME", "/home/sage"),
            ("PATH", "/usr/bin"),
            ("CLAUDE_CONFIG_DIR", "")
        ]),
        Ok(vec![
            (
                "CLAUDE_CONFIG_DIR".into(),
                PathBuf::from("/home/sage").join(".claude").into_os_string()
            ),
            ("HOME".into(), "/home/sage".into()),
            ("PATH".into(), "/usr/bin".into()),
        ])
    );
    assert_eq!(
        derived(&[("PATH", "/usr/bin")]),
        Err(ClaudeAdmissionUnavailable::CandidateInvalid)
    );
}

#[test]
fn admitted_runtime_launch_contract_scopes_state_and_environment() {
    let root = std::env::temp_dir().join(format!("sage-claude-admission-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let adapter = root.join("adapter.js");
    let node = root.join("node.exe");
    fs::write(&adapter, b"adapter").unwrap();
    fs::write(&node, b"node").unwrap();
    let candidate = DiscoveredClaudeLaunch {
        adapter_path: adapter.clone(),
        adapter_version: "0.76.0".into(),
        acp_sdk_version: "1.4.0".into(),
        node_engine: ">=22".into(),
        node_path: node.clone(),
        node_version: "22.18.0".into(),
        adapter_sha256: crate::harness_discovery::sha256_file(&adapter).unwrap(),
        node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
    };
    let registration = ClaudeBindingRegistration::admitted_for_test(
        ClaudeLaunchDiscovery::Present(Box::new(candidate)),
    );
    let binding = registration.refresh().unwrap();
    #[cfg(windows)]
    let expected = {
        let profile = std::env::var_os("USERPROFILE").unwrap();
        let config = std::env::var_os("CLAUDE_CONFIG_DIR")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| PathBuf::from(&profile).join(".claude").into_os_string());
        vec![
            ("CLAUDE_CONFIG_DIR".into(), config),
            ("USERPROFILE".into(), profile),
            ("APPDATA".into(), std::env::var_os("APPDATA").unwrap()),
            (
                "LOCALAPPDATA".into(),
                std::env::var_os("LOCALAPPDATA").unwrap(),
            ),
            ("PATH".into(), std::env::var_os("PATH").unwrap()),
            ("PATHEXT".into(), std::env::var_os("PATHEXT").unwrap()),
            ("ComSpec".into(), std::env::var_os("ComSpec").unwrap()),
            (
                "SYSTEMROOT".into(),
                super::admission::system_windows_directory_for_test()
                    .expect("Windows directory authority"),
            ),
            ("TEMP".into(), std::env::var_os("TEMP").unwrap()),
            ("TMP".into(), std::env::var_os("TMP").unwrap()),
        ]
    };
    #[cfg(not(windows))]
    let expected = Vec::new();
    assert_eq!(binding.launch_environment().unwrap(), expected);
    assert_eq!(binding.candidate().runtime_path, node);
    assert_eq!(binding.candidate().artifact_path, adapter);
    fs::remove_file(&adapter).unwrap();
    fs::remove_file(&node).unwrap();
    assert_eq!(binding.launch_environment().unwrap(), expected);
    assert_eq!(binding.revalidate(), Err(ClaudeAdmissionUnavailable::Drift));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn admitted_claude_runtime_launch_spec_preserves_the_exact_launch_contract() {
    use crate::acp::bindings::runtime::RuntimeFactory;

    let root = std::env::temp_dir().join(format!("sage-claude-runtime-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create root");
    let adapter = root.join("adapter.js");
    let node = root.join("node.exe");
    fs::write(&adapter, b"adapter").expect("write adapter");
    fs::write(&node, b"node").expect("write node");
    let registration = ClaudeBindingRegistration::admitted_for_test(
        ClaudeLaunchDiscovery::Present(Box::new(DiscoveredClaudeLaunch {
            adapter_path: adapter.clone(),
            adapter_version: "0.76.0".into(),
            acp_sdk_version: "1.4.0".into(),
            node_engine: ">=22".into(),
            node_path: node.clone(),
            node_version: "22.18.0".into(),
            adapter_sha256: crate::harness_discovery::sha256_file(&adapter)
                .expect("adapter digest"),
            node_sha256: crate::harness_discovery::sha256_file(&node).expect("node digest"),
        })),
    );
    let binding = registration.refresh().expect("admitted binding");
    let root_authority = crate::project_registry::ProjectRootAuthority::for_test(
        SageProjectId::parse("project-claude-runtime").expect("project"),
        ProjectGeneration::ZERO,
        SageRootId::parse("root-claude-runtime").expect("root"),
        &root,
    )
    .expect("root authority");
    let mut runtime = Box::new(ClaudeRuntimeFactory::from_admitted_binding(
        binding.clone(),
        root_authority,
    ))
    .construct(
        RuntimeEpoch::new(11).expect("epoch"),
        SageAgentSessionId::parse("session-claude-launch").expect("session"),
    )
    .expect("runtime");
    let spec = runtime.launch_spec().expect("launch spec");
    let mut binding_id = [0_u8; 16];
    binding_id.copy_from_slice(&binding.binding_digest()[..16]);
    let expected_node_digest: [u8; 32] = sha2::Sha256::digest(b"node").into();
    assert_eq!(spec.binding_id, Uuid::from_bytes(binding_id));
    assert_eq!(spec.runtime_epoch, 11);
    assert_eq!(spec.artifact.canonical_absolute_path, node);
    assert_eq!(spec.artifact.sha256, expected_node_digest);
    assert_eq!(spec.artifact.byte_len, 4);
    assert_eq!(
        spec.fixed_args
            .as_slice()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec![adapter.to_string_lossy().into_owned()]
    );
    assert_eq!(
        spec.environment.as_slice(),
        binding.launch_environment().unwrap()
    );
    assert_eq!(
        spec.project_root.scope().0.as_str(),
        "project-claude-runtime"
    );
    assert_eq!(spec.project_root.scope().2.as_str(), "root-claude-runtime");
    spec.project_root
        .revalidate()
        .expect("retained project root");
    assert_eq!(
        spec.required_guarantee,
        if cfg!(windows) {
            crate::acp::process::RequiredContainment::EnforcedTree
        } else {
            crate::acp::process::RequiredContainment::TrustedCooperativeTree
        }
    );
    assert_eq!(spec.limits, crate::acp::process::TreeLimits::ade_0006a());
    assert_eq!(
        spec.deadlines,
        crate::acp::process::ProcessDeadlines::ade_0006a()
    );
    assert!(spec.absolute_cleanup_deadline.is_none());
    drop(spec);
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn fetch_tool_retains_only_an_admitted_loopback_browse_url() {
    let start = json!({
        "method": "session/update",
        "params": {
            "sessionId": "claude-session-1",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "browse-1",
                "kind": "fetch",
                "title": "_sage/browse",
                "status": "pending",
                "locations": [{"url": "http://127.0.0.1:5173/app"}],
                "rawInput": {"url": "https://example.com/secret"}
            }
        }
    });
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("prompt-browse").unwrap();
    let events = translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();
    match &past_turn_start(&events)[0] {
        AdapterEvent::Item {
            payload:
                NormalizedPayload::ToolCall {
                    category: ToolCategory::Fetch,
                    input_summary: Some(summary),
                    ..
                },
            ..
        } => assert_eq!(summary.as_str(), "http://127.0.0.1:5173/app"),
        other => panic!("expected browse summary, got {other:?}"),
    }
    let projected = format!("{events:?}");
    assert!(!projected.contains("example.com"));
    assert!(!projected.contains("secret"));
}

const MAPPED_FIXTURE_PATH: &str = r"C:\fixture\root\src\lib.rs";

fn diff_item_count(events: &[AdapterEvent], key: &str) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(event, AdapterEvent::Item { external_key, .. } if external_key.as_str() == key)
        })
        .count()
}

fn tool_frame(value: Value) -> Value {
    json!({
        "method": "session/update",
        "params": {"sessionId": "claude-session-1", "update": value},
    })
}

#[test]
fn a_second_late_diff_frame_after_settlement_adds_no_further_terminal_item() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("late-diff-prompt").unwrap();
    let start = tool_frame(json!({
        "sessionUpdate": "tool_call", "toolCallId": "late-1",
        "kind": "edit", "status": "pending"
    }));
    translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();
    let settle = tool_frame(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "late-1", "status": "completed"
    }));
    translator
        .translate_notification(&encoded(&settle), &FixtureResources)
        .unwrap();
    let late = |new_text: &str| {
        tool_frame(json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "late-1", "status": "completed",
            "content": [{
                "type": "diff", "path": MAPPED_FIXTURE_PATH,
                "oldText": "a\n", "newText": new_text
            }]
        }))
    };
    let first = translator
        .translate_notification(&encoded(&late("b\n")), &FixtureResources)
        .unwrap();
    assert_eq!(diff_item_count(&first, "late-1:diff:0"), 1);
    let second = translator
        .translate_notification(&encoded(&late("c\nd\n")), &FixtureResources)
        .unwrap();
    assert_eq!(diff_item_count(&second, "late-1:diff:0"), 0);
}

#[test]
fn a_binding_denied_its_terminal_event_by_the_frame_budget_settles_on_a_later_frame() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("budget-prompt").unwrap();
    let start = tool_frame(json!({
        "sessionUpdate": "tool_call", "toolCallId": "cap-1", "kind": "edit", "status": "pending",
        "content": [
            {"type": "diff", "path": r"C:\elsewhere\one.rs", "oldText": "a\n", "newText": "b\n"},
            {"type": "diff", "path": r"C:\elsewhere\two.rs", "oldText": "a\n", "newText": "b\n"}
        ]
    }));
    translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();
    let mut content = vec![
        json!({"type": "content", "content": {"type": "text", "text": "x"}}),
        json!({"type": "content", "content": {"type": "text", "text": "y"}}),
    ];
    for _ in 0..7 {
        content.push(json!({
            "type": "diff", "path": MAPPED_FIXTURE_PATH,
            "oldText": "a\n", "newText": "b\n"
        }));
    }
    let settle = tool_frame(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "cap-1",
        "status": "completed", "content": content
    }));
    let settled = translator
        .translate_notification(&encoded(&settle), &FixtureResources)
        .unwrap();
    let later = tool_frame(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "cap-1",
        "status": "completed", "title": "Edit again"
    }));
    let drained = translator
        .translate_notification(&encoded(&later), &FixtureResources)
        .unwrap();
    let keys: Vec<String> = (0..9).map(|index| format!("cap-1:diff:{index}")).collect();
    let deferred = keys
        .iter()
        .filter(|key| diff_item_count(&settled, key) == 0)
        .count();
    assert!(
        deferred > 0,
        "expected the frame budget to defer a terminal summary"
    );
    for key in &keys {
        assert_eq!(
            diff_item_count(&settled, key) + diff_item_count(&drained, key),
            1,
            "{key} terminalized exactly once"
        );
    }
}

#[test]
fn a_declined_write_settles_its_held_creation_without_failing_the_frame() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("decline-prompt").unwrap();
    let tool = "toolu_01DeclineWriteFixture000001";
    let created = r"C:\fixture\root\src\created.rs";
    let start = tool_frame(json!({
        "sessionUpdate": "tool_call", "toolCallId": tool,
        "kind": "edit", "status": "pending",
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "rawInput": {"file_path": created, "content": "body\n"},
        "title": "Write created.rs",
        "content": [{"type": "diff", "path": created, "oldText": Value::Null, "newText": "body\n"}],
        "locations": [{"path": created}],
    }));
    translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();

    translator
        .translate_permission_request_value(json!({
            "id": 41,
            "method": "session/request_permission",
            "params": {
                "sessionId": "claude-session-1",
                "toolCall": {
                    "toolCallId": tool,
                    "kind": "edit",
                    "status": "pending",
                    "title": "Write created.rs",
                },
                "options": [
                    {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
                    {"optionId": "reject", "name": "No", "kind": "reject_once"},
                ],
            },
        }))
        .unwrap();
    translator.permission_decline_committed("41").unwrap();

    let declined = tool_frame(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": tool, "status": "failed",
        "_meta": {"claudeCode": {
            "nonExecutionKind": "fileOperations",
            "toolName": "Write",
        }},
        "rawOutput": "The user doesn't want to take this action right",
        "content": [{"type": "content", "content": {
            "type": "text",
            "text": "The user doesn't want to proceed with this tool use. Stopped.",
        }}],
    }));
    let events = translator
        .translate_notification(&encoded(&declined), &FixtureResources)
        .unwrap();

    let diff_key = format!("{tool}:diff:0");
    assert_eq!(diff_item_count(&events, &diff_key), 0);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AdapterEvent::FilePair { .. }))
    );
    let drained = translator
        .translate_prompt_result_value(json!({
            "id": "decline-prompt",
            "result": {"stopReason": "end_turn"},
        }))
        .unwrap();
    assert_eq!(diff_item_count(&drained, &diff_key), 1);
}

#[test]
fn a_decline_committed_after_a_failed_write_absorbs_the_late_resolution() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("decline-race-prompt").unwrap();
    let tool = "toolu_01DeclineRaceFixture000001";
    let created = r"C:\fixture\root\src\raced.rs";
    let start = tool_frame(json!({
        "sessionUpdate": "tool_call", "toolCallId": tool,
        "kind": "edit", "status": "pending",
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "title": "Write raced.rs",
        "content": [{"type": "diff", "path": created, "oldText": Value::Null, "newText": "body\n"}],
        "locations": [{"path": created}],
    }));
    translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();
    translator
        .translate_permission_request_value(json!({
            "id": 42,
            "method": "session/request_permission",
            "params": {
                "sessionId": "claude-session-1",
                "toolCall": {
                    "toolCallId": tool,
                    "kind": "edit",
                    "status": "pending",
                    "title": "Write raced.rs",
                },
                "options": [
                    {"optionId": "reject", "name": "No", "kind": "reject_once"},
                ],
            },
        }))
        .unwrap();

    let failed = tool_frame(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": tool, "status": "failed",
        "rawOutput": "The user doesn't want to take this action right",
        "content": [{"type": "content", "content": {
            "type": "text",
            "text": "The user doesn't want to proceed with this tool use. Stopped.",
        }}],
    }));
    let events = translator
        .translate_notification(&encoded(&failed), &FixtureResources)
        .unwrap();
    let diff_key = format!("{tool}:diff:0");
    assert_eq!(diff_item_count(&events, &diff_key), 1);

    assert!(matches!(
        translator.permission_decline_committed("42").unwrap(),
        AdapterEvent::IgnoredDuplicate
    ));
    let drained = translator
        .translate_notification(&encoded(&failed), &FixtureResources)
        .unwrap();
    assert_eq!(diff_item_count(&drained, &diff_key), 0);
}

fn declined_write(tool: &str) -> ClaudeTranslator {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("decline-ledger-prompt").unwrap();
    let start = tool_frame(json!({
        "sessionUpdate": "tool_call", "toolCallId": tool,
        "kind": "edit", "status": "pending",
        "title": "Write lib.rs",
        "content": [{
            "type": "diff",
            "path": MAPPED_FIXTURE_PATH,
            "oldText": "one\n",
            "newText": "two\n",
        }],
    }));
    translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();
    translator
        .translate_permission_request_value(json!({
            "id": 77,
            "method": "session/request_permission",
            "params": {
                "sessionId": "claude-session-1",
                "toolCall": {
                    "toolCallId": tool,
                    "kind": "edit",
                    "status": "pending",
                    "title": "Write lib.rs",
                },
                "options": [{"optionId": "reject", "name": "No", "kind": "reject_once"}],
            },
        }))
        .unwrap();
    translator.permission_decline_committed("77").unwrap();
    translator
}

fn reported_status(tool: &str, status: &str) -> Vec<u8> {
    encoded(&tool_frame(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": tool,
        "status": status,
    })))
}

#[test]
fn a_repeated_terminal_report_after_a_decline_stays_absorbed() {
    let tool = "toolu_01DeclineLedgerRepeat0001";
    let mut translator = declined_write(tool);
    let failed = reported_status(tool, "failed");
    translator
        .translate_notification(&failed, &FixtureResources)
        .unwrap();
    assert!(matches!(
        translator
            .translate_notification(&failed, &FixtureResources)
            .unwrap()
            .as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
}

#[test]
fn a_conflicting_terminal_report_after_a_decline_tears_down() {
    let tool = "toolu_01DeclineLedgerClash00001";
    let mut translator = declined_write(tool);
    translator
        .translate_notification(&reported_status(tool, "failed"), &FixtureResources)
        .unwrap();
    assert!(
        translator
            .translate_notification(&reported_status(tool, "completed"), &FixtureResources)
            .is_err()
    );
}

#[test]
fn an_in_progress_report_after_a_decline_is_absorbed() {
    let tool = "toolu_01DeclineLedgerProgress01";
    let mut translator = declined_write(tool);
    assert!(matches!(
        translator
            .translate_notification(&reported_status(tool, "in_progress"), &FixtureResources)
            .unwrap()
            .as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
}

#[test]
fn a_decline_with_no_later_harness_frame_publishes_its_diff_at_turn_end() {
    let tool = "toolu_01DeclineLedgerQuiet00001";
    let mut translator = declined_write(tool);
    let events = translator
        .translate_prompt_result_value(json!({
            "id": "decline-ledger-prompt",
            "result": {"stopReason": "end_turn"},
        }))
        .unwrap();
    assert_eq!(diff_item_count(&events, &format!("{tool}:diff:0")), 1);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AdapterEvent::FilePair { .. }))
    );
}

#[test]
fn an_unsettled_diff_publishes_its_verified_pair_when_the_turn_finalizes() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("drain-prompt").unwrap();
    let tool = "toolu_01DrainFixture00000000001";
    let start = tool_frame(json!({
        "sessionUpdate": "tool_call", "toolCallId": tool,
        "kind": "edit", "status": "in_progress",
        "content": [{
            "type": "diff",
            "path": MAPPED_FIXTURE_PATH,
            "oldText": "one\n",
            "newText": "two\n",
        }],
    }));
    translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();

    let events = translator
        .translate_prompt_result_value(json!({
            "id": "drain-prompt",
            "result": {"stopReason": "end_turn"},
        }))
        .unwrap();
    assert_eq!(diff_item_count(&events, &format!("{tool}:diff:0")), 1);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AdapterEvent::FilePair { .. }))
    );
}

#[test]
fn a_post_terminal_title_change_never_mutates_the_settled_tool_call() {
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("post-terminal-prompt").unwrap();
    let tool = "toolu_01PostTerminalFixture0001";
    let start = tool_frame(json!({
        "sessionUpdate": "tool_call", "toolCallId": tool,
        "kind": "read", "status": "completed", "title": "Read lib.rs",
    }));
    translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();

    let renamed = tool_frame(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": tool,
        "status": "completed", "title": "Read lib.rs (retitled)",
    }));
    assert!(matches!(
        translator
            .translate_notification(&encoded(&renamed), &FixtureResources)
            .unwrap()
            .as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
}

struct CreationResources(std::cell::Cell<bool>);

impl crate::acp::bindings::adapter::ResourceMapper for CreationResources {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        self.map_absolute(absolute_path)
    }
}

impl ResourceMapper for CreationResources {
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        if !self.0.get() {
            return Err(UnavailableResourceReason::Foreign);
        }
        let name = absolute_path
            .rsplit('\\')
            .next()
            .ok_or(UnavailableResourceReason::Foreign)?;
        SafeResourceRef::new(
            SageRootId::parse("root-fixture")
                .map_err(|_| UnavailableResourceReason::Unsupported)?,
            format!("src/{name}"),
        )
        .map_err(|_| UnavailableResourceReason::Unsupported)
    }
}

#[test]
fn creations_beyond_the_frame_budget_are_promoted_on_a_later_frame_not_stranded() {
    let resources = CreationResources(std::cell::Cell::new(false));
    let mut translator = ClaudeTranslator::new(claude_profile(), "claude-session-1").unwrap();
    translator.begin_prompt("creation-budget-prompt").unwrap();
    let content = (0..9)
        .map(|index| {
            json!({
                "type": "diff",
                "path": format!(r"C:\fixture\root\src\new{index}.rs"),
                "newText": "created\n"
            })
        })
        .collect::<Vec<_>>();
    let start = tool_frame(json!({
        "sessionUpdate": "tool_call", "toolCallId": "many-1", "kind": "edit",
        "status": "pending", "content": content
    }));
    translator
        .translate_notification(&encoded(&start), &resources)
        .unwrap();
    resources.0.set(true);
    let settle = tool_frame(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "many-1", "status": "completed"
    }));
    let settled = translator
        .translate_notification(&encoded(&settle), &resources)
        .unwrap();
    let promoted = (0..9)
        .filter(|index| diff_item_count(&settled, &format!("many-1:diff:{index}")) == 1)
        .count();
    assert!(
        promoted < 9,
        "expected the frame budget to defer some creations"
    );
    let later = tool_frame(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "many-1",
        "status": "completed", "title": "Write again"
    }));
    let drained = translator
        .translate_notification(&encoded(&later), &resources)
        .unwrap();
    for index in 0..9 {
        let key = format!("many-1:diff:{index}");
        assert_eq!(
            diff_item_count(&settled, &key) + diff_item_count(&drained, &key),
            1,
            "diff {index} terminalized exactly once"
        );
    }
    let pairs = drained
        .iter()
        .filter(|event| matches!(event, AdapterEvent::FilePair { .. }))
        .count();
    assert_eq!(
        pairs,
        9 - promoted,
        "deferred creations promote as openable pairs"
    );
}

#[cfg(windows)]
fn absolute(name: &str) -> PathBuf {
    PathBuf::from(format!(r"C:\sage-h3-fixture\{name}"))
}

#[cfg(not(windows))]
fn absolute(name: &str) -> PathBuf {
    PathBuf::from(format!("/sage-h3-fixture/{name}"))
}
