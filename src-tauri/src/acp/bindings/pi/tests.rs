use serde_json::{Value, json};

use super::decode::{FRAME_BYTES, ITEM_BYTES, MODEL_BYTES, MODEL_NAME_BYTES};
use super::{
    AdapterError, AdapterOutput, ArtifactCandidate, ArtifactIdentity, CuratedPiBinding,
    HistoryRole, Operation, PI_LICENSE, PI_NODE_MINIMUM, PI_PACKAGE, PI_RPC_ARGS, PI_SOURCE_COMMIT,
    PI_VERSION, PiBindingError, PiPlatform, PiTranslator, TerminalOutcome, ToolOutcome,
};

fn identity(path: &str, byte: char) -> ArtifactIdentity {
    ArtifactIdentity {
        relative_path: path.into(),
        sha256: byte.to_string().repeat(64),
        bytes: 42,
    }
}

fn research_candidate() -> (CuratedPiBinding, ArtifactCandidate) {
    let entrypoint = identity("pi/dist/cli.js", 'a');
    let runtime = identity("node/node.exe", 'b');
    (
        CuratedPiBinding {
            platform: PiPlatform::WindowsX64,
            entrypoint: entrypoint.clone(),
            runtime: runtime.clone(),
            node_version: "22.19.0".into(),
        },
        ArtifactCandidate {
            platform: PiPlatform::WindowsX64,
            package_name: PI_PACKAGE.into(),
            package_version: PI_VERSION.into(),
            source_commit: PI_SOURCE_COMMIT.into(),
            license: PI_LICENSE.into(),
            node_version: "22.19.0".into(),
            entrypoint,
            runtime,
            fixed_args: PI_RPC_ARGS.map(str::to_owned).to_vec(),
        },
    )
}

fn response(id: &str, command: &str, data: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "type": "response",
        "id": id,
        "command": command,
        "success": true,
        "data": data
    }))
    .unwrap()
}

fn failed_response(id: &str, command: &str, data: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "type": "response",
        "id": id,
        "command": command,
        "success": false,
        "data": data
    }))
    .unwrap()
}

fn accept_prompt(translator: &mut PiTranslator, turn: &str) -> String {
    let outbound = translator
        .begin(Operation::Prompt {
            turn_key: turn.into(),
            message: "Inspect without writing.".into(),
        })
        .unwrap()
        .remove(0);
    let outputs = translator
        .ingest(&response(&outbound.command_id, "prompt", Value::Null))
        .unwrap();
    assert_eq!(
        outputs,
        vec![AdapterOutput::CommandAccepted { command: "prompt" }]
    );
    outbound.command_id
}

fn admitted_translator(epoch: u64) -> PiTranslator {
    PiTranslator::fixture_admitted(epoch)
}

#[test]
fn exact_manifest_admission_is_fail_closed_and_capability_honest() {
    assert_eq!(PI_NODE_MINIMUM, "22.19.0");
    let _supported_platforms = [
        PiPlatform::WindowsX64,
        PiPlatform::LinuxX64,
        PiPlatform::LinuxArm64,
        PiPlatform::MacOsX64,
        PiPlatform::MacOsArm64,
    ];
    let (curated, mut candidate) = research_candidate();
    let binding = curated.validate_candidate(&candidate).unwrap();
    let capabilities = binding.capabilities();
    assert!(!capabilities.prompt() && !capabilities.abort() && capabilities.entries());
    assert!(capabilities.initialize());
    assert!(capabilities.model_discovery());
    assert!(capabilities.new_session());
    assert!(capabilities.state());
    assert!(capabilities.messages());
    assert!(!capabilities.resume());
    assert!(!capabilities.load());
    assert!(!capabilities.close());
    assert!(!capabilities.multi_root());
    assert!(!capabilities.fork_or_clone());
    assert!(!capabilities.write_tools());

    candidate.entrypoint.sha256 = "A".repeat(64);
    assert!(matches!(
        curated.validate_candidate(&candidate),
        Err(PiBindingError::IdentityMismatch("entrypoint identity"))
            | Err(PiBindingError::InvalidCuratedMetadata("sha256"))
    ));
    assert_eq!(
        binding.require_launch_admission(),
        Err(PiBindingError::ArtifactGraphNotAdmitted)
    );
}

#[test]
fn canonical_relative_artifact_paths_reject_aliases_and_absolute_paths() {
    for path in ["../pi/cli.js", "pi/./cli.js", "C:/pi/cli.js", "/pi/cli.js"] {
        let (mut curated, candidate) = research_candidate();
        curated.entrypoint.relative_path = path.into();
        assert!(matches!(
            curated.validate_candidate(&candidate),
            Err(PiBindingError::InvalidCuratedMetadata(_))
        ));
    }
}

#[test]
fn initialization_correlates_state_and_models_in_either_order() {
    let mut translator = PiTranslator::unavailable(7);
    let outbound = translator.begin(Operation::Initialize).unwrap();
    assert_eq!(
        translator.begin(Operation::Initialize),
        Err(AdapterError::InitializationAlreadyActive)
    );
    assert_eq!(outbound.len(), 2);
    for command in &outbound {
        assert!(!command.line.contains(['\r', '\n']));
        assert!(serde_json::from_str::<Value>(&command.line).is_ok());
        // The shared writer owns exactly one trailing LF.
        assert!(command.line.len() < FRAME_BYTES);
    }
    let models = response(
        &outbound[1].command_id,
        "get_available_models",
        json!({"models":[{"provider":"openai","id":"gpt","name":"GPT"}]}),
    );
    let first = translator.ingest(&models).unwrap();
    assert!(matches!(
        first.as_slice(),
        [
            AdapterOutput::Models(_),
            AdapterOutput::InitializationProgress
        ]
    ));
    let state = response(
        &outbound[0].command_id,
        "get_state",
        json!({"model":{"provider":"openai","id":"gpt"},"sessionFile":"C:\\private\\session.jsonl"}),
    );
    let second = translator.ingest(&state).unwrap();
    assert!(matches!(
        second.as_slice(),
        [AdapterOutput::State(_), AdapterOutput::Initialized]
    ));
    let serialized = format!("{second:?}");
    assert!(!serialized.contains("C:\\private"));
    assert_eq!(translator.begin(Operation::Initialize).unwrap().len(), 2);
}

#[test]
fn prompt_acceptance_does_not_complete_turn_and_fixture_orders_items() {
    let mut translator = admitted_translator(1);
    let prompt_id = accept_prompt(&mut translator, "turn-1");
    let duplicate = translator
        .ingest(&response(&prompt_id, "prompt", Value::Null))
        .unwrap();
    assert_eq!(duplicate, vec![AdapterOutput::DuplicateIgnored]);

    let fixture = include_str!("../../../../testdata/acp/pi/read_only_turn.jsonl");
    let mut outputs = Vec::new();
    for line in fixture.lines() {
        outputs.extend(translator.ingest(format!("{line}\r\n").as_bytes()).unwrap());
    }
    assert!(matches!(outputs[0], AdapterOutput::TurnRunning { .. }));
    assert!(outputs.iter().any(|output| matches!(
        output,
        AdapterOutput::ToolTerminal {
            outcome: ToolOutcome::Completed,
            ..
        }
    )));
    assert!(outputs.iter().any(|output| matches!(
        output,
        AdapterOutput::TurnTerminal {
            outcome: TerminalOutcome::Completed,
            ..
        }
    )));
    let duplicate_terminal = translator.ingest(br#"{"type":"agent_settled"}"#).unwrap();
    assert_eq!(duplicate_terminal, vec![AdapterOutput::DuplicateIgnored]);
    let late = translator
        .ingest(br#"{"type":"message_update","delta":"late"}"#)
        .unwrap();
    assert_eq!(late, vec![AdapterOutput::LateIgnored]);
}

#[test]
fn captured_pi_1_5_turn_projects_thinking_text_and_tool_without_unknown_events() {
    let mut translator = admitted_translator(9);
    accept_prompt(&mut translator, "turn-capture");
    let capture = include_str!("../../../../testdata/acp/pi/captured_session_150.jsonl");
    let mut outputs = Vec::new();
    for line in capture
        .lines()
        .skip_while(|line| !line.contains(r#""type":"agent_start""#))
    {
        outputs.extend(translator.ingest(line.as_bytes()).unwrap());
        if line.contains(r#""type":"agent_settled""#) {
            break;
        }
    }
    let unknown = |output: &AdapterOutput| matches!(output, AdapterOutput::Unsupported { .. });
    assert!(!outputs.iter().any(unknown), "{outputs:?}");
    assert!(outputs.contains(&AdapterOutput::ThoughtDelta {
        text: "Thinking about it.".into(),
        redacted_tail: false,
    }));
    assert!(outputs.contains(&AdapterOutput::TextDelta {
        text: "I will inspect.".into(),
        redacted_tail: false,
    }));
    assert!(outputs.iter().any(|output| matches!(
        output,
        AdapterOutput::ToolTerminal {
            outcome: ToolOutcome::Completed,
            ..
        }
    )));
    assert!(matches!(
        outputs.last(),
        Some(AdapterOutput::TurnTerminal {
            outcome: TerminalOutcome::Completed,
            ..
        })
    ));
}

#[test]
fn write_or_shell_observation_blocks_without_normalizing_arguments() {
    let mut translator = admitted_translator(2);
    accept_prompt(&mut translator, "turn-2");
    translator.ingest(br#"{"type":"agent_start"}"#).unwrap();
    let output = translator
        .ingest(
            br#"{"type":"tool_execution_start","toolCallId":"tool-write","toolName":"bash","args":{"command":"rm -rf C:\\Users\\real"}}"#,
        )
        .unwrap();
    assert_eq!(
        output,
        vec![AdapterOutput::AuthorityBlocked {
            external_id: "tool-write".into(),
            tool: "bash".into()
        }]
    );
    assert!(!format!("{output:?}").contains("Users"));

    assert_eq!(
        translator.ingest(br#"{"type":"message_update","delta":"must not pass"}"#),
        Ok(vec![AdapterOutput::LateIgnored])
    );
}

#[test]
fn duplicate_tool_start_cannot_reopen_a_terminal_tool() {
    let mut translator = admitted_translator(8);
    accept_prompt(&mut translator, "turn-tool");
    translator.ingest(br#"{"type":"agent_start"}"#).unwrap();
    let start = br#"{"type":"tool_execution_start","toolCallId":"tool-1","toolName":"read"}"#;
    let end = br#"{"type":"tool_execution_end","toolCallId":"tool-1","isError":false}"#;
    translator.ingest(start).unwrap();
    assert!(matches!(
        translator.ingest(end).unwrap().as_slice(),
        [AdapterOutput::ToolTerminal {
            outcome: ToolOutcome::Completed,
            ..
        }]
    ));
    assert_eq!(
        translator.ingest(start).unwrap(),
        vec![AdapterOutput::DuplicateIgnored]
    );
    assert_eq!(
        translator.ingest(end).unwrap(),
        vec![AdapterOutput::DuplicateIgnored]
    );
}

#[test]
fn pi_compaction_start_and_end_correlate_one_notice_lifecycle() {
    let mut translator = admitted_translator(8);
    accept_prompt(&mut translator, "turn-compaction");
    translator.ingest(br#"{"type":"agent_start"}"#).unwrap();
    assert_eq!(
        translator
            .ingest(br#"{"type":"compaction_start","reason":"threshold"}"#)
            .unwrap(),
        vec![AdapterOutput::CompactionStarted]
    );
    assert_eq!(
        translator
            .ingest(br#"{"type":"compaction_start","reason":"threshold"}"#)
            .unwrap(),
        vec![AdapterOutput::DuplicateIgnored]
    );
    assert_eq!(
        translator
            .ingest(br#"{"type":"compaction_end","reason":"threshold","result":{"summary":"Earlier context","firstKeptEntryId":"entry-1","tokensBefore":1000},"aborted":false,"willRetry":false}"#)
            .unwrap(),
        vec![AdapterOutput::CompactionCompleted]
    );
    assert_eq!(
        translator
            .ingest(br#"{"type":"compaction_end","reason":"threshold","result":{"summary":"Earlier context","firstKeptEntryId":"entry-1","tokensBefore":1000},"aborted":false,"willRetry":false}"#)
            .unwrap(),
        vec![AdapterOutput::LateIgnored]
    );
}

#[test]
fn pi_turn_end_without_compaction_end_interrupts_the_notice_first() {
    let mut translator = admitted_translator(8);
    accept_prompt(&mut translator, "turn-compaction");
    translator.ingest(br#"{"type":"agent_start"}"#).unwrap();
    translator
        .ingest(br#"{"type":"compaction_start","reason":"threshold"}"#)
        .unwrap();
    translator
        .ingest(br#"{"type":"agent_end","messages":[],"willRetry":false}"#)
        .unwrap();
    let events = translator.ingest(br#"{"type":"agent_settled"}"#).unwrap();
    assert!(matches!(
        events.as_slice(),
        [
            AdapterOutput::CompactionInterrupted,
            AdapterOutput::TurnTerminal { .. }
        ]
    ));
}

#[test]
fn abort_capacity_failure_does_not_mark_an_unsent_interrupt() {
    let mut translator = admitted_translator(9);
    accept_prompt(&mut translator, "turn-abort");
    translator.ingest(br#"{"type":"agent_start"}"#).unwrap();
    for _ in 0..16 {
        translator.begin(Operation::GetState).unwrap();
    }
    assert_eq!(
        translator.begin(Operation::Abort),
        Err(AdapterError::CommandCapacity)
    );
    translator
        .ingest(br#"{"type":"agent_end","messages":[],"willRetry":false}"#)
        .unwrap();
    let terminal = translator.ingest(br#"{"type":"agent_settled"}"#).unwrap();
    assert!(matches!(
        terminal.as_slice(),
        [AdapterOutput::TurnTerminal {
            outcome: TerminalOutcome::Completed,
            ..
        }]
    ));
}

#[test]
fn denial_latch_survives_abort_enqueue_and_response_failures() {
    let partial = json!({"role":"assistant","content":[{"type":"text","text":"blocked"}],
        "api":"openai-responses","provider":"openai","model":"fixture-model","timestamp":0,
        "stopReason":"stop","usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,
            "totalTokens":0,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}}});
    let update = serde_json::to_vec(&json!({"type":"message_update","message":partial,
        "assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"blocked","partial":partial}})).unwrap();
    let mut translator = admitted_translator(17);
    accept_prompt(&mut translator, "turn-denied-full");
    translator.ingest(br#"{"type":"agent_start"}"#).unwrap();
    for _ in 0..16 {
        translator.begin(Operation::GetState).unwrap();
    }
    let denied = translator
        .ingest(br#"{"type":"tool_execution_start","toolCallId":"tool-write","toolName":"write"}"#)
        .unwrap();
    assert!(matches!(
        denied.as_slice(),
        [AdapterOutput::AuthorityBlocked { .. }]
    ));
    assert_eq!(
        translator.begin(Operation::Abort),
        Err(AdapterError::CommandCapacity)
    );
    assert_eq!(
        translator.ingest(&update),
        Ok(vec![AdapterOutput::LateIgnored])
    );
    translator
        .ingest(br#"{"type":"agent_end","messages":[],"willRetry":false}"#)
        .unwrap();
    assert!(matches!(
        translator
            .ingest(br#"{"type":"agent_settled"}"#)
            .unwrap()
            .as_slice(),
        [AdapterOutput::TurnTerminal {
            outcome: TerminalOutcome::Interrupted,
            ..
        }]
    ));

    let mut translator = admitted_translator(18);
    accept_prompt(&mut translator, "turn-denied-response");
    translator.ingest(br#"{"type":"agent_start"}"#).unwrap();
    translator
        .ingest(br#"{"type":"extension_ui_request","id":"gate-1","method":"confirm","title":"Allow tool?","message":"Allow write?"}"#)
        .unwrap();
    let abort = translator.begin(Operation::Abort).unwrap().remove(0);
    translator.latch_authority_denial();
    assert_eq!(
        translator
            .ingest(&failed_response(&abort.command_id, "abort", Value::Null))
            .unwrap(),
        vec![AdapterOutput::Error {
            code: "command_failed"
        }]
    );
    assert_eq!(
        translator.ingest(&update),
        Ok(vec![AdapterOutput::LateIgnored])
    );
}

#[test]
fn id_less_events_require_the_exact_prompt_start_terminal_grammar() {
    let mut translator = admitted_translator(19);
    assert_eq!(
        translator.ingest(br#"{"type":"agent_start"}"#),
        Ok(vec![AdapterOutput::LateIgnored])
    );

    let first = translator
        .begin(Operation::Prompt {
            turn_key: "turn-old".into(),
            message: "read only".into(),
        })
        .unwrap()
        .remove(0);
    for event in [
        br#"{"type":"agent_start"}"#.as_slice(),
        br#"{"type":"message_update","delta":"early"}"#.as_slice(),
        br#"{"type":"agent_end"}"#.as_slice(),
    ] {
        assert_eq!(translator.ingest(event), Err(AdapterError::EventSequence));
    }
    translator
        .ingest(&response(&first.command_id, "prompt", Value::Null))
        .unwrap();
    assert_eq!(
        translator.ingest(br#"{"type":"agent_end"}"#),
        Err(AdapterError::EventSequence)
    );
    translator.ingest(br#"{"type":"agent_start"}"#).unwrap();
    assert_eq!(
        translator.ingest(br#"{"type":"agent_start"}"#),
        Err(AdapterError::EventSequence)
    );
    translator.ingest(br#"{"type":"agent_end"}"#).unwrap();
    translator.ingest(br#"{"type":"agent_settled"}"#).unwrap();

    accept_prompt(&mut translator, "turn-new");
    assert_eq!(
        translator.ingest(br#"{"type":"agent_end"}"#),
        Err(AdapterError::EventSequence)
    );
    translator.ingest(br#"{"type":"agent_start"}"#).unwrap();
    translator
        .ingest(br#"{"type":"agent_end","messages":[],"willRetry":false}"#)
        .unwrap();
    assert!(matches!(
        translator.ingest(br#"{"type":"agent_settled"}"#).unwrap().as_slice(),
        [AdapterOutput::TurnTerminal { turn_key, .. }] if turn_key == "turn-new"
    ));
}

#[test]
fn failed_or_malformed_initialization_is_cleared_for_retry() {
    let mut translator = PiTranslator::unavailable(20);
    let first = translator.begin(Operation::Initialize).unwrap();
    assert!(matches!(
        translator
            .ingest(&failed_response(
                &first[0].command_id,
                "get_state",
                Value::Null
            ))
            .unwrap()
            .as_slice(),
        [AdapterOutput::Error { .. }]
    ));
    assert_eq!(translator.pending_count(), 0);
    assert_eq!(translator.begin(Operation::Initialize).unwrap().len(), 2);

    let mut translator = PiTranslator::unavailable(21);
    let second = translator.begin(Operation::Initialize).unwrap();
    assert_eq!(
        translator.ingest(&response(
            &second[1].command_id,
            "get_available_models",
            json!({"models":[{"id":"model","name":"x".repeat(MODEL_NAME_BYTES + 1)}]})
        )),
        Err(AdapterError::Limit("text bytes"))
    );
    assert_eq!(translator.pending_count(), 0);
    assert_eq!(translator.begin(Operation::Initialize).unwrap().len(), 2);
}

#[test]
fn correlated_failures_are_bounded_and_never_expose_provider_payloads() {
    let mut translator = admitted_translator(11);
    let prompt = translator
        .begin(Operation::Prompt {
            turn_key: "turn-failed".into(),
            message: "Read only".into(),
        })
        .unwrap()
        .remove(0);
    let output = translator
        .ingest(&failed_response(
            &prompt.command_id,
            "prompt",
            json!({"error":"Bearer top-secret C:\\Users\\private"}),
        ))
        .unwrap();
    assert!(matches!(
        output.as_slice(),
        [
            AdapterOutput::Error {
                code: "command_failed"
            },
            AdapterOutput::TurnTerminal {
                outcome: TerminalOutcome::Failed,
                ..
            }
        ]
    ));
    let rendered = format!("{output:?}");
    assert!(!rendered.contains("top-secret"));
    assert!(!rendered.contains("Users"));
}

#[test]
fn new_session_is_serialized_behind_pending_or_active_work() {
    let mut translator = admitted_translator(12);
    translator.begin(Operation::GetState).unwrap();
    assert_eq!(
        translator.begin(Operation::NewSession),
        Err(AdapterError::SessionTransitionBusy)
    );

    let mut translator = admitted_translator(13);
    accept_prompt(&mut translator, "turn-live");
    assert_eq!(
        translator.begin(Operation::NewSession),
        Err(AdapterError::SessionTransitionBusy)
    );
}

#[test]
fn pending_commands_are_bounded_at_exact_and_plus_one() {
    let mut translator = PiTranslator::unavailable(3);
    for _ in 0..16 {
        translator.begin(Operation::GetState).unwrap();
    }
    assert_eq!(translator.pending_count(), 16);
    assert_eq!(
        translator.begin(Operation::GetState),
        Err(AdapterError::CommandCapacity)
    );
}

#[test]
fn entry_queries_are_correlated_and_protocol_terminal_variants_remain_explicit() {
    let mut translator = PiTranslator::unavailable(26);
    let command = translator
        .begin(Operation::GetEntries {
            since: Some("cursor-1".into()),
        })
        .unwrap()
        .remove(0);
    let output = translator
        .ingest(&response(
            &command.command_id,
            "get_entries",
            json!({"entries":[{"id":"entry-1","type":"message"}]}),
        ))
        .unwrap();
    assert!(matches!(
        output.as_slice(),
        [AdapterOutput::Entries(entries)] if entries.len() == 1
    ));
    let _normalized_terminal_states = [ToolOutcome::Declined, ToolOutcome::Cancelled];
    assert!(matches!(
        AdapterError::PermissionViolation,
        AdapterError::PermissionViolation
    ));
}

#[test]
fn model_and_history_pages_reject_plus_one_and_redact_absolute_paths() {
    let mut translator = PiTranslator::unavailable(4);
    let models_command = translator.begin(Operation::Initialize).unwrap().remove(1);
    let models = (0..129)
        .map(|index| json!({"provider":"p","id":format!("m-{index}")}))
        .collect::<Vec<_>>();
    assert_eq!(
        translator.ingest(&response(
            &models_command.command_id,
            "get_available_models",
            json!({"models":models})
        )),
        Err(AdapterError::Limit("model count"))
    );

    let mut translator = PiTranslator::unavailable(5);
    let command = translator.begin(Operation::GetMessages).unwrap().remove(0);
    let output = translator
        .ingest(&response(
            &command.command_id,
            "get_messages",
            json!({"messages":[{"role":"assistant","content":"See C:\\Users\\real\\secret.txt and /home/real/.env"}]})
        ))
        .unwrap();
    let AdapterOutput::Messages(messages) = &output[0] else {
        panic!("expected messages");
    };
    assert_eq!(messages[0].role, HistoryRole::Assistant);
    assert!(!messages[0].text.contains("Users"));
    assert!(!messages[0].text.contains("/home"));

    let mut translator = PiTranslator::unavailable(10);
    let command = translator.begin(Operation::GetMessages).unwrap().remove(0);
    let output = translator
        .ingest(&response(
            &command.command_id,
            "get_messages",
            json!({"messages":[{"role":"assistant","content":"Authorization: Bearer top-secret sk-canary"}]})
        ))
        .unwrap();
    assert!(!format!("{output:?}").contains("top-secret"));
    assert!(!format!("{output:?}").contains("sk-canary"));
}

#[test]
fn model_and_message_byte_budgets_accept_exact_and_reject_plus_one() {
    assert_eq!(MODEL_BYTES, 262_144);
    assert_eq!(ITEM_BYTES, 262_144);
    assert_eq!(MODEL_NAME_BYTES, 8_192);

    let exact_models = (0..32)
        .map(|_| json!({"id":"m","name":"x".repeat(8_191)}))
        .collect::<Vec<_>>();
    let mut translator = PiTranslator::unavailable(22);
    let command = translator.begin(Operation::Initialize).unwrap().remove(1);
    assert!(matches!(
        translator
            .ingest(&response(
                &command.command_id,
                "get_available_models",
                json!({"models":exact_models})
            ))
            .unwrap()
            .as_slice(),
        [AdapterOutput::Models(models), AdapterOutput::InitializationProgress]
            if models.len() == 32
    ));

    let plus_one_models = (0..32)
        .map(|index| {
            let name_bytes = if index == 31 { 8_192 } else { 8_191 };
            json!({"id":"m","name":"x".repeat(name_bytes)})
        })
        .collect::<Vec<_>>();
    let mut translator = PiTranslator::unavailable(23);
    let command = translator.begin(Operation::Initialize).unwrap().remove(1);
    assert_eq!(
        translator.ingest(&response(
            &command.command_id,
            "get_available_models",
            json!({"models":plus_one_models})
        )),
        Err(AdapterError::Limit("model bytes"))
    );

    for (epoch, text_bytes, expected) in [
        (24, ITEM_BYTES, None),
        (
            25,
            ITEM_BYTES + 1,
            Some(AdapterError::Limit("message bytes")),
        ),
    ] {
        let mut translator = PiTranslator::unavailable(epoch);
        let command = translator.begin(Operation::GetMessages).unwrap().remove(0);
        let result = translator.ingest(&response(
            &command.command_id,
            "get_messages",
            json!({"messages":[{"role":"assistant","content":[
                {"type":"text","text":"x".repeat(text_bytes / 2)},
                {"type":"text","text":"x".repeat(text_bytes - text_bytes / 2)}
            ]}]}),
        ));
        match expected {
            None => assert!(matches!(
                result.unwrap().as_slice(),
                [AdapterOutput::Messages(_)]
            )),
            Some(error) => assert_eq!(result, Err(error)),
        }
    }
}

#[test]
fn frame_and_history_boundaries_accept_exact_and_reject_plus_one() {
    let prefix = br#"{"type":"future_event","padding":""#;
    let suffix = br#""}"#;
    let mut exact = Vec::with_capacity(FRAME_BYTES);
    exact.extend_from_slice(prefix);
    exact.resize(FRAME_BYTES - suffix.len(), b'a');
    exact.extend_from_slice(suffix);
    let mut translator = PiTranslator::unavailable(14);
    assert_eq!(
        translator.ingest(&exact),
        Ok(vec![AdapterOutput::LateIgnored])
    );
    exact.insert(exact.len() - suffix.len(), b'a');
    assert_eq!(
        translator.ingest(&exact),
        Err(AdapterError::Limit("frame bytes"))
    );

    let mut translator = PiTranslator::unavailable(15);
    let command = translator.begin(Operation::GetMessages).unwrap().remove(0);
    let messages = (0..256)
        .map(|_| json!({"role":"assistant","content":"x"}))
        .collect::<Vec<_>>();
    let output = translator
        .ingest(&response(
            &command.command_id,
            "get_messages",
            json!({"messages":messages}),
        ))
        .unwrap();
    assert!(matches!(output.as_slice(), [AdapterOutput::Messages(values)] if values.len() == 256));

    let mut translator = PiTranslator::unavailable(16);
    let command = translator.begin(Operation::GetMessages).unwrap().remove(0);
    let messages = (0..257)
        .map(|_| json!({"role":"assistant","content":"x"}))
        .collect::<Vec<_>>();
    assert_eq!(
        translator.ingest(&response(
            &command.command_id,
            "get_messages",
            json!({"messages":messages})
        )),
        Err(AdapterError::Limit("message count"))
    );
}

#[test]
fn malformed_unknown_foreign_and_unsupported_inputs_are_deterministic() {
    let mut translator = PiTranslator::unavailable(6);
    assert_eq!(
        translator.ingest(b"not-json\n"),
        Err(AdapterError::Malformed)
    );
    assert_eq!(
        translator
            .ingest(br#"{"type":"response","id":"foreign","command":"get_state","success":true}"#),
        Err(AdapterError::Correlation)
    );
    assert_eq!(
        translator.ingest(br#"{"type":"future_event","payload":{"secret":"sk-private"}}"#),
        Ok(vec![AdapterOutput::LateIgnored])
    );
    for operation in [
        Operation::Load,
        Operation::Close,
        Operation::MultiRoot,
        Operation::ForkOrClone,
    ] {
        assert!(matches!(
            translator.begin(operation),
            Err(AdapterError::UnsupportedOperation(_))
        ));
    }
    assert!(matches!(
        translator.begin(Operation::Prompt {
            turn_key: "turn".into(),
            message: "hello".into()
        }),
        Err(AdapterError::LaunchNotAdmitted)
    ));
}
