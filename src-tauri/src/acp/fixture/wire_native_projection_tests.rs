use crate::acp::{
    bindings::standard_acp::{
        profile_for,
        translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator},
    },
    contract::SageRootId,
    fixture::wire::capture::{replay_native, replay_native_before},
    intent::{
        NormalizedPayload, PermissionOptionKind, SafeResourceLocation, SafeResourceRef,
        ToolCategory, UnavailableResourceReason,
    },
    lifecycle::{ItemLifecycle, ToolState},
};
use serde_json::{Value, json};

const COMMAND: &str = r"printf '%s\n' 'Inspect synthetic example 1'";

struct ExistingFile(std::path::PathBuf);

impl ExistingFile {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-native-edit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/example.ts"), "export const example = 1;\n").unwrap();
        Self(root)
    }

    fn path(&self) -> String {
        self.0.join("src/example.ts").to_str().unwrap().to_owned()
    }
}

impl Drop for ExistingFile {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

impl ResourceMapper for ExistingFile {
    fn map_absolute(&self, path: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        if path != self.path() || !std::path::Path::new(path).is_file() {
            return Err(UnavailableResourceReason::Foreign);
        }
        Ok(
            SafeResourceRef::new(SageRootId::parse("native-root").unwrap(), "src/example.ts")
                .unwrap(),
        )
    }
}

impl crate::acp::bindings::codex::ResourceMapper for ExistingFile {
    fn map_existing(&self, path: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        self.map_absolute(path)
    }
}

fn translator() -> StandardAcpTranslator {
    let mut translator = StandardAcpTranslator::new(
        profile_for("claude-code").unwrap(),
        "fixture-persisted-session-1",
    )
    .unwrap();
    translator.begin_prompt("3").unwrap();
    translator
}

fn client_requests(requests: &[Value]) -> String {
    requests
        .iter()
        .map(|request| format!("{request}\n"))
        .collect()
}

fn requests(tail: &[Value]) -> String {
    client_requests(&[
        json!({"id":1,"method":"initialize"}),
        json!({"id":2,"method":"session/new"}),
        json!({"id":3,"method":"session/prompt"}),
    ]) + &client_requests(tail)
}

#[test]
fn approval_wire_projects_nonempty_setup_and_correlates_permission_only_to_second_prompt() {
    for (case, option, expected_state) in [
        (
            "claude-approval-approved",
            "fixture-allow-once",
            ToolState::Completed,
        ),
        (
            "claude-approval-declined",
            "fixture-reject",
            ToolState::Failed,
        ),
        (
            "claude-approval-pending",
            "fixture-allow-once",
            ToolState::Completed,
        ),
    ] {
        let pending = case == "claude-approval-pending";
        let mut tail = vec![
            json!({"id":4,"method":"session/set_mode","params":{"modeId":"default"}}),
            json!({"id":5,"method":"session/prompt"}),
        ];
        if !pending {
            tail.extend([
                json!({"id":"fixture-permission-1","result":{"outcome":{"outcome":"selected","optionId":option}}}),
                json!({"id":6,"method":"session/close"}),
            ]);
        }
        let frames = if pending {
            replay_native_before(case, &requests(&tail), "acpPermissionDecision")
        } else {
            replay_native(case, &requests(&tail))
        };
        let setup = frames.iter().position(|frame| frame["id"] == 3).unwrap();
        let permission = frames
            .iter()
            .position(|frame| frame["method"] == "session/request_permission")
            .unwrap();
        let terminal = frames
            .iter()
            .position(|frame| frame["id"] == 5)
            .unwrap_or(frames.len());
        assert!(setup < permission && permission < terminal);
        assert_eq!(frames[setup]["result"]["stopReason"], "end_turn");
        if !pending {
            assert_eq!(frames[terminal]["result"]["stopReason"], "end_turn");
            let completion = &frames[terminal - 1]["params"]["update"];
            let (output, fence) = if expected_state == ToolState::Failed {
                (
                    "Synthetic approval declined; no command was executed.",
                    "```",
                )
            } else {
                (
                    "Synthetic tool finished; no command was executed.",
                    "```console",
                )
            };
            assert_eq!(completion["rawOutput"], output);
            assert_eq!(
                completion["content"],
                json!([{"type":"content","content":{
                    "type":"text","text":format!("{fence}\n{output}\n```")
                }}])
            );
            assert_eq!(
                completion["_meta"],
                json!({"claudeCode":{"toolName":"Bash"}})
            );
        }
        assert_eq!(
            frames[permission]["params"]["toolCall"]["rawInput"]["command"],
            COMMAND
        );
        assert_eq!(frames[permission]["params"]["toolCall"]["title"], "Bash");
        assert_eq!(frames[permission]["params"]["toolCall"]["name"], "Bash");
        assert_eq!(
            frames[permission]["params"]["toolCall"]["status"],
            "pending"
        );
        assert_eq!(
            frames[permission]["params"]["toolCall"]["content"],
            json!([])
        );
        assert!(
            frames[permission]["params"]["toolCall"]
                .get("_meta")
                .is_none()
        );
        assert_eq!(
            frames[permission]["params"]["_meta"],
            json!({"permission":{"version":1,"title":"Bash"}})
        );
        let file = ExistingFile::new();
        let mut projection = translator();
        let setup_events = projection
            .translate_notification_value(frames[setup - 1].clone(), &file)
            .unwrap();
        assert!(setup_events.iter().any(|event| matches!(event,
            AdapterEvent::Item { payload: NormalizedPayload::Text { text }, .. }
            if text.as_str() == "Synthetic approval setup completed. No tool was run."
        )));
        projection
            .translate_prompt_result_value(frames[setup].clone())
            .unwrap();
        projection.begin_prompt("5").unwrap();
        let mut tools = Vec::new();
        for frame in &frames[setup + 1..terminal] {
            if frame["method"] == "session/update" {
                tools.extend(
                    projection
                        .translate_notification_value(frame.clone(), &file)
                        .unwrap(),
                );
            } else if frame["method"] == "session/request_permission" {
                let cards = projection
                    .translate_permission_request_value(frame.clone())
                    .unwrap();
                let [
                    AdapterEvent::Item {
                        external_key,
                        lifecycle: ItemLifecycle::Streaming,
                        tool_state: Some(ToolState::WaitingForApproval),
                        payload:
                            NormalizedPayload::PermissionRequest {
                                category: ToolCategory::Execute,
                                summary,
                                offered_option_kinds: Some(options),
                            },
                    },
                ] = cards.as_slice()
                else {
                    panic!("expected one pending approval card");
                };
                assert_eq!(external_key.as_str(), "permission:fixture-permission-1");
                assert_eq!(summary.as_str(), COMMAND);
                assert_eq!(
                    options.as_slice(),
                    [
                        PermissionOptionKind::AllowOnce,
                        PermissionOptionKind::AllowAlways,
                        PermissionOptionKind::RejectOnce
                    ]
                );
                tools.extend(cards);
            }
        }
        if pending {
            assert!(!tools.iter().any(|event| matches!(
                event,
                AdapterEvent::TurnTerminal(_)
                    | AdapterEvent::Item {
                        tool_state: Some(ToolState::Completed | ToolState::Failed),
                        ..
                    }
            )));
            assert!(frames.iter().all(|frame| frame["id"] != 5));
            continue;
        }
        assert!(tools.iter().any(|event| matches!(event,
            AdapterEvent::Item { tool_state: Some(state), payload: NormalizedPayload::ToolCall { input_summary: Some(input), .. }, .. }
            if *state == expected_state && input.as_str() == COMMAND
        )));
        projection
            .translate_prompt_result_value(frames[terminal].clone())
            .unwrap();
    }
}

#[test]
fn claude_bash_wire_projects_command_lifecycle_without_fabricated_file_pairs_or_diffs() {
    let frames = replay_native(
        "claude-tool-expanded",
        &requests(&[json!({"id":4,"method":"session/close"})]),
    );
    let mut projection = translator();
    let file = ExistingFile::new();
    let mut states = Vec::new();
    for frame in frames
        .into_iter()
        .filter(|frame| frame["method"] == "session/update")
    {
        for event in projection
            .translate_notification_value(frame, &file)
            .unwrap()
        {
            assert!(!matches!(
                event,
                AdapterEvent::FilePair { .. }
                    | AdapterEvent::Item {
                        payload: NormalizedPayload::FileChangeDiff { .. },
                        ..
                    }
            ));
            if let AdapterEvent::Item {
                tool_state: Some(state),
                payload: NormalizedPayload::ToolCall { input_summary, .. },
                ..
            } = event
            {
                assert_eq!(input_summary.unwrap().as_str(), COMMAND);
                states.push(state);
            }
        }
    }
    assert_eq!(
        states,
        [
            ToolState::Pending,
            ToolState::InProgress,
            ToolState::Completed
        ]
    );
}

#[test]
fn live_claude_edit_projects_openable_resource_without_a_completed_pair() {
    let frames = replay_native(
        "claude-file-edit-live",
        &requests(&[
            json!({"method":"session/cancel"}),
            json!({"id":4,"method":"session/close"}),
        ]),
    );
    let file = ExistingFile::new();
    let mut projection = translator();
    let mut events = Vec::new();
    for mut frame in frames
        .into_iter()
        .filter(|frame| frame["method"] == "session/update")
    {
        if let Some(path) = frame.pointer_mut("/params/update/locations/0/path") {
            *path = json!(file.path());
        }
        if let Some(path) = frame.pointer_mut("/params/update/content/0/path") {
            *path = json!(file.path());
        }
        events.extend(
            projection
                .translate_notification_value(frame, &file)
                .unwrap(),
        );
    }
    let states: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Item {
                tool_state: Some(state),
                payload: NormalizedPayload::ToolCall { .. },
                ..
            } => Some(*state),
            _ => None,
        })
        .collect();
    assert_eq!(states, [ToolState::Pending, ToolState::InProgress]);
    assert!(events.iter().any(|event| matches!(event,
        AdapterEvent::Item { lifecycle: ItemLifecycle::Streaming,
            payload: NormalizedPayload::FileChangeDiff { resource: SafeResourceLocation::Resource { resource }, pair_available: false, additions: Some(1), deletions: Some(1), .. }, .. }
        if resource.relative_path.as_str() == "src/example.ts"
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AdapterEvent::FilePair { .. }))
    );
    assert_eq!(
        std::fs::read_to_string(file.path()).unwrap(),
        "export const example = 1;\n"
    );
}

#[test]
fn completed_claude_edit_projects_the_existing_file_and_exact_retained_pair() {
    let frames = replay_native(
        "claude-file-diff-tab",
        &requests(&[json!({"id":4,"method":"session/close"})]),
    );
    let file = ExistingFile::new();
    let mut projection = translator();
    let mut events = Vec::new();
    for mut frame in frames
        .into_iter()
        .filter(|frame| frame["method"] == "session/update")
    {
        if let Some(path) = frame.pointer_mut("/params/update/locations/0/path") {
            *path = json!(file.path());
        }
        if let Some(path) = frame.pointer_mut("/params/update/content/0/path") {
            *path = json!(file.path());
        }
        events.extend(
            projection
                .translate_notification_value(frame, &file)
                .unwrap(),
        );
    }
    assert!(events.iter().any(|event| matches!(event,
        AdapterEvent::Item { lifecycle: ItemLifecycle::Completed,
            payload: NormalizedPayload::FileChangeDiff { resource: SafeResourceLocation::Resource { resource }, pair_available: true, .. }, .. }
        if resource.relative_path.as_str() == "src/example.ts"
    )));
    let pairs: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::FilePair {
                old_text, new_text, ..
            } => Some((old_text.as_str(), new_text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        pairs,
        [("export const example = 1;\n", "export const example = 2;\n")]
    );
}

#[test]
fn codex_live_file_wire_allows_live_projection_without_fabricated_completed_pair_or_diff() {
    let frames = replay_native(
        "codex-file-edit-live",
        &client_requests(&[
            json!({"id":1,"method":"initialize"}),
            json!({"method":"initialized"}),
            json!({"id":2,"method":"thread/start"}),
            json!({"id":3,"method":"thread/goal/get"}),
            json!({"id":4,"method":"turn/start"}),
            json!({"id":5,"method":"turn/interrupt"}),
            json!({"id":6,"method":"thread/unsubscribe"}),
        ]),
    );
    let frame = frames
        .iter()
        .find(|frame| frame["method"] == "item/started")
        .unwrap();
    let file = ExistingFile::new();
    let mut item = frame["params"]["item"].clone();
    item["changes"][0]["path"] = json!(file.path());
    assert_eq!(item["status"], "inProgress");
    if let Some((payload, state, terminal)) =
        crate::acp::bindings::codex::normalize_transcript_item(
            item.as_object().unwrap(),
            ItemLifecycle::Started,
            &file,
        )
        .unwrap()
    {
        assert!(!terminal.is_some_and(|lifecycle| lifecycle.is_terminal()));
        assert_ne!(state, Some(ToolState::Completed));
        assert!(!matches!(
            payload,
            NormalizedPayload::FileChangeDiff {
                pair_available: true,
                ..
            }
        ));
    }
    assert!(
        !frames
            .iter()
            .any(|frame| frame["method"] == "item/completed")
    );
}
