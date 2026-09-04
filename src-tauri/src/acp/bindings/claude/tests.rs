use std::path::PathBuf;

use serde_json::{Value, json};

use super::*;
use crate::acp::{
    contract::SageRootId,
    intent::{
        IMAGE_BASE64_MAX, ImageMediaType, NormalizedPayload, SafeResourceRef, ToolCategory,
        UnavailableResourceReason,
    },
    lifecycle::{ItemLifecycle, ToolState, TurnState},
    limits::LIMITS,
};

const PNG_1X1: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

struct FixtureResources;

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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
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
            payload: NormalizedPayload::Image { media_type, data },
            ..
        },
    ] = events.as_slice()
    else {
        panic!("expected one admitted image, got {events:?}")
    };
    assert_eq!(*media_type, ImageMediaType::Png);
    assert_eq!(data.as_str(), PNG_1X1);
}

#[test]
fn agent_image_chunk_rejects_svg_uri_oversize_and_missing_bytes() {
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
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
                events.as_slice(),
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    for content in [
        json!({ "type": "audio", "mimeType": "audio/wav", "data": "AAAA" }),
        json!({ "type": "resource", "resource": { "uri": "file:///tmp/x" } }),
    ] {
        let events = translator
            .translate_notification(&encoded(&image_chunk(content)), &FixtureResources)
            .unwrap();
        assert!(matches!(
            events.as_slice(),
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
        ] = events
        else {
            panic!("expected one text snapshot")
        };
        text.as_str()
    }

    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
    translator.begin_prompt("prompt-expected").unwrap();
    let foreign = json!({ "id": "prompt-foreign", "result": { "stopReason": "end_turn" } });
    assert!(matches!(
        translator.translate_prompt_result(&encoded(&foreign)),
        Err(TranslationError::ForeignId)
    ));
    let expected = json!({ "id": "prompt-expected", "result": { "stopReason": "end_turn" } });
    assert!(matches!(
        translator
            .translate_prompt_result(&encoded(&expected))
            .unwrap()
            .as_slice(),
        [AdapterEvent::TurnTerminal(PromptTerminal::Completed)]
    ));
}

#[test]
fn one_session_runs_two_ordered_turns_and_retains_old_terminal_idempotency() {
    let first = json!({ "id": "prompt-1", "result": { "stopReason": "end_turn" } });
    let second = json!({ "id": "prompt-2", "result": { "stopReason": "cancelled" } });
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    assert!(matches!(
        translator
            .translate_prompt_result(&encoded(&first))
            .unwrap()
            .as_slice(),
        [AdapterEvent::TurnTerminal(PromptTerminal::Completed)]
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
        [AdapterEvent::TurnTerminal(PromptTerminal::Cancelled)]
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
    translator.begin_prompt("prompt-error").unwrap();
    let events = translator
        .translate_prompt_result(&encoded(&error))
        .unwrap();
    assert!(matches!(
        events.as_slice(),
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
    let start = translator
        .translate_notification(&encoded(&frames[0]), &FixtureResources)
        .unwrap();
    assert!(matches!(
        &start[0],
        AdapterEvent::Item {
            lifecycle: ItemLifecycle::Started,
            tool_state: Some(ToolState::Pending),
            payload: NormalizedPayload::ToolCall {
                category: ToolCategory::File,
                input_summary: None,
                ..
            },
            ..
        }
    ));

    let completed = translator
        .translate_notification(&encoded(&frames[1]), &FixtureResources)
        .unwrap();
    assert_eq!(completed.len(), 2);
    assert!(matches!(
        &completed[0],
        AdapterEvent::Item {
            lifecycle: ItemLifecycle::Completed,
            tool_state: Some(ToolState::Completed),
            payload: NormalizedPayload::ToolUpdateResult {
                output_summary: None,
                ..
            },
            ..
        }
    ));
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
    let projected = format!("{start:?}{completed:?}");
    assert!(!projected.contains("secret-command"));
    assert!(!projected.contains("sk-ant-fixture"));
    assert!(!projected.contains(r"C:\fixture"));
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
    translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();
    let events = translator
        .translate_permission_request(fixture_bytes("permission"))
        .unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        AdapterEvent::Item {
            tool_state: Some(ToolState::WaitingForApproval),
            payload: NormalizedPayload::PermissionRequest { .. },
            ..
        }
    ));
    assert_eq!(events.len(), 1);
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    assert!(matches!(
        translator
            .translate_prompt_result(&encoded(&terminal))
            .unwrap()
            .as_slice(),
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
fn unproven_thought_and_nested_transcript_updates_are_bounded_fallbacks() {
    let frames = fixture("unsupported").as_array().unwrap().clone();
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
    for frame in frames {
        let events = translator
            .translate_notification(&encoded(&frame), &FixtureResources)
            .unwrap();
        assert!(matches!(
            &events[0],
            AdapterEvent::Item {
                lifecycle: ItemLifecycle::Unsupported,
                payload: NormalizedPayload::Unsupported { .. },
                ..
            }
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
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
fn research_manifest_is_exact_bounded_single_root_and_never_launchable() {
    let candidate = CandidateManifest {
        source_commit: "e56f344691a56c07e5dae2ebeb6ad2a6416f8c9d".into(),
        adapter_version: "0.64.0".into(),
        acp_sdk_version: "1.3.0".into(),
        claude_sdk_version: "0.3.220".into(),
        license: "Apache-2.0".into(),
        artifact_path: absolute("claude-adapter.js"),
        artifact_sha256: "a".repeat(64),
        runtime_path: absolute("node.exe"),
        runtime_sha256: "b".repeat(64),
        isolated_state_dir: absolute("isolated-state"),
        fixed_arguments: vec!["claude-adapter.js".into()],
        environment_keys: vec!["CLAUDE_CONFIG_DIR".into()],
        project_root_count: 1,
    };
    let validated = validate_research_candidate(candidate.clone(), &[]).unwrap();
    assert!(matches!(
        validated.require_launch_admission(),
        Err(ManifestError::ArtifactGraphNotAdmitted)
    ));
    let mut multi_root = candidate.clone();
    multi_root.project_root_count = 2;
    assert!(matches!(
        validate_research_candidate(multi_root, &[]),
        Err(ManifestError::MultiRootUnsupported)
    ));
    let forbidden = vec![candidate.isolated_state_dir.clone()];
    assert!(matches!(
        validate_research_candidate(candidate, &forbidden),
        Err(ManifestError::AmbientState)
    ));
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
    let mut translator = ClaudeTranslator::new("claude-session-1").unwrap();
    translator.begin_prompt("prompt-browse").unwrap();
    let events = translator
        .translate_notification(&encoded(&start), &FixtureResources)
        .unwrap();
    match &events[0] {
        AdapterEvent::Item {
            payload:
                NormalizedPayload::ToolCall {
                    category: ToolCategory::Web,
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

#[cfg(windows)]
fn absolute(name: &str) -> PathBuf {
    PathBuf::from(format!(r"C:\sage-h3-fixture\{name}"))
}

#[cfg(not(windows))]
fn absolute(name: &str) -> PathBuf {
    PathBuf::from(format!("/sage-h3-fixture/{name}"))
}
