//! Waves 6-8 native capture replays (scripts/acp-native-parity-events.mjs):
//! the fork RPCs, MCP / web-search / mixed tool rows and second Turns answer
//! the bound client exactly and project through the real bindings.

use crate::acp::{
    bindings::standard_acp::{
        profile_for,
        translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator},
    },
    fixture::wire::capture::replay_native,
    intent::{NormalizedPayload, SafeResourceRef, ToolCategory, UnavailableResourceReason},
    lifecycle::{ItemLifecycle, ToolState},
};
use serde_json::{Value, json};

const SESSION: &str = "fixture-persisted-session-1";
const FORK: &str = "fixture-fork-session-1";

struct NoFiles;

impl ResourceMapper for NoFiles {
    fn map_absolute(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

impl crate::acp::bindings::codex::ResourceMapper for NoFiles {
    fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

fn lines(requests: &[Value]) -> String {
    requests
        .iter()
        .map(|request| format!("{request}\n"))
        .collect()
}

/// A Codex capture's client side through the first Turn, then `tail`.
fn codex_requests(tail: &[Value]) -> String {
    lines(&[
        json!({"id":1,"method":"initialize"}),
        json!({"method":"initialized"}),
        json!({"id":2,"method":"thread/start"}),
        json!({"id":3,"method":"thread/goal/get"}),
        json!({"id":4,"method":"turn/start"}),
    ]) + &lines(tail)
}

fn claude_requests(tail: &[Value]) -> String {
    lines(&[
        json!({"id":1,"method":"initialize"}),
        json!({"id":2,"method":"session/new"}),
        json!({"id":3,"method":"session/prompt"}),
    ]) + &lines(tail)
}

fn response(frames: &[Value], id: u64) -> &Value {
    frames
        .iter()
        .find(|frame| frame["id"] == id && frame.get("method").is_none())
        .unwrap_or_else(|| panic!("no response {id}"))
}

/// Each completed Codex item normalized by the real Codex binding.
fn codex_tools(case: &str) -> Vec<(ToolCategory, Option<ToolState>, Option<String>)> {
    let frames = replay_native(
        case,
        &codex_requests(&[json!({"id":5,"method":"thread/unsubscribe"})]),
    );
    frames
        .iter()
        .filter(|frame| frame["method"] == "item/completed")
        .filter_map(|frame| {
            crate::acp::bindings::codex::normalize_transcript_item(
                frame["params"]["item"].as_object().unwrap(),
                ItemLifecycle::Completed,
                &NoFiles,
            )
            .unwrap()
        })
        .filter_map(|(payload, state, _)| match payload {
            NormalizedPayload::ToolCall {
                category,
                output_summary,
                ..
            } => Some((
                category,
                state,
                output_summary.map(|value| value.as_str().to_owned()),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn codex_fork_thread_answers_thread_fork_with_its_lineage_and_acks_the_release() {
    let frames = replay_native(
        "codex-fork-thread",
        &codex_requests(&[
            json!({"id":5,"method":"thread/fork","params":{"threadId":SESSION}}),
            json!({"id":6,"method":"thread/unsubscribe","params":{"threadId":FORK}}),
            json!({"id":7,"method":"thread/unsubscribe","params":{"threadId":SESSION}}),
        ]),
    );
    let completed = frames
        .iter()
        .position(|frame| frame["method"] == "turn/completed")
        .unwrap();
    let fork = frames.iter().position(|frame| frame["id"] == 5).unwrap();
    assert!(completed < fork, "the fork follows the settled Turn");
    let thread = &response(&frames, 5)["result"]["thread"];
    assert_eq!(thread["id"], FORK);
    assert_eq!(thread["forkedFromId"], SESSION);
    assert_eq!(response(&frames, 6)["result"], json!({}));
    assert_eq!(response(&frames, 7)["result"], json!({}));
}

#[test]
fn codex_answer_menu_and_dialog_cases_never_answer_a_fork() {
    for case in ["codex-fork-menu", "codex-fork-dialog"] {
        let frames = replay_native(
            case,
            &codex_requests(&[json!({"id":5,"method":"thread/unsubscribe"})]),
        );
        assert!(
            frames
                .iter()
                .all(|frame| frame["result"]["thread"]["id"] != FORK)
        );
    }
}

#[test]
fn claude_fork_cases_advertise_unstable_fork_and_answer_one_session_fork() {
    let frames = replay_native(
        "claude-fork-thread",
        &claude_requests(&[
            json!({"id":4,"method":"session/fork","params":{"sessionId":SESSION,"cwd":"C:\\fixture"}}),
            json!({"id":5,"method":"session/close"}),
        ]),
    );
    let initialize = serde_json::to_vec(response(&frames, 1)).unwrap();
    let offer =
        crate::acp::bindings::claude::ClaudeCapabilityOffer::from_initialize_bytes(&initialize)
            .unwrap();
    assert!(offer.fork_session);
    assert!(offer.close_session);
    assert_eq!(response(&frames, 4)["result"]["sessionId"], FORK);
    // Non-fork Claude cases keep the established initialize offer.
    let plain = replay_native(
        "claude-turn-completed",
        &claude_requests(&[json!({"id":4,"method":"session/close"})]),
    );
    let plain = serde_json::to_vec(response(&plain, 1)).unwrap();
    assert!(
        !crate::acp::bindings::claude::ClaudeCapabilityOffer::from_initialize_bytes(&plain)
            .unwrap()
            .fork_session
    );
}

#[test]
fn codex_mcp_cases_project_a_json_result_and_an_error_message_only() {
    let result = codex_tools("codex-tool-mcp-result");
    assert_eq!(result.len(), 1);
    let (category, state, output) = &result[0];
    assert_eq!(*category, ToolCategory::Mcp);
    assert_eq!(*state, Some(ToolState::Completed));
    let parsed: Value = serde_json::from_str(output.as_deref().unwrap()).unwrap();
    assert_eq!(parsed["hits"][0]["title"], "Synthetic doc");
    let error = codex_tools("codex-tool-mcp-error");
    assert_eq!(error.len(), 1);
    assert_eq!(error[0].0, ToolCategory::Mcp);
    assert_eq!(error[0].1, Some(ToolState::Failed));
    assert_eq!(error[0].2.as_deref(), Some("Synthetic MCP probe error"));
}

#[test]
fn codex_web_search_and_mixed_cases_project_their_distinct_kinds() {
    let web = codex_tools("codex-tool-web-search");
    assert_eq!(
        web.iter()
            .map(|(category, ..)| *category)
            .collect::<Vec<_>>(),
        [ToolCategory::WebSearch]
    );
    let mixed = codex_tools("codex-tool-mixed-group");
    assert_eq!(
        mixed
            .iter()
            .map(|(category, ..)| *category)
            .collect::<Vec<_>>(),
        [
            ToolCategory::Execute,
            ToolCategory::WebSearch,
            ToolCategory::Mcp
        ]
    );
}

#[test]
fn claude_web_search_and_mixed_cases_classify_from_protocol_fields() {
    for (case, expected) in [
        ("claude-tool-web-search", vec![ToolCategory::WebSearch]),
        (
            "claude-tool-mixed-group",
            vec![ToolCategory::Execute, ToolCategory::WebSearch],
        ),
    ] {
        let frames = replay_native(
            case,
            &claude_requests(&[json!({"id":4,"method":"session/close"})]),
        );
        let mut translator =
            StandardAcpTranslator::new(profile_for("claude-code").unwrap(), SESSION).unwrap();
        translator.begin_prompt("3").unwrap();
        let mut categories = Vec::new();
        for frame in frames
            .into_iter()
            .filter(|frame| frame["method"] == "session/update")
        {
            for event in translator
                .translate_notification_value(frame, &NoFiles)
                .unwrap()
            {
                if let AdapterEvent::Item {
                    payload: NormalizedPayload::ToolCall { category, .. },
                    tool_state: Some(ToolState::Pending),
                    ..
                } = event
                {
                    categories.push(category);
                }
            }
        }
        assert_eq!(categories, expected, "{case}");
    }
}

#[test]
fn second_turn_cases_answer_two_prompts_with_distinct_turns() {
    for case in ["codex-annotation-sent", "codex-turn-navigator-hover"] {
        let frames = replay_native(
            case,
            &codex_requests(&[
                json!({"id":5,"method":"turn/start"}),
                json!({"id":6,"method":"thread/unsubscribe"}),
            ]),
        );
        let first = &response(&frames, 4)["result"]["turn"]["id"];
        let second = &response(&frames, 5)["result"]["turn"]["id"];
        assert_ne!(first, second, "{case}");
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame["method"] == "turn/completed")
                .count(),
            2,
            "{case}"
        );
    }
    let claude = replay_native(
        "claude-annotation-sent",
        &claude_requests(&[
            json!({"id":4,"method":"session/prompt"}),
            json!({"id":5,"method":"session/close"}),
        ]),
    );
    assert_eq!(response(&claude, 3)["result"]["stopReason"], "end_turn");
    assert_eq!(response(&claude, 4)["result"]["stopReason"], "end_turn");
}

#[test]
fn codex_subagent_list_case_keeps_one_child_done_and_one_running() {
    let frames = replay_native(
        "codex-subagents-tab-active-done",
        &codex_requests(&[
            json!({"id":5,"method":"turn/interrupt"}),
            json!({"id":6,"method":"thread/unsubscribe"}),
        ]),
    );
    let states: Vec<_> = frames
        .iter()
        .filter(|frame| frame["params"]["item"]["type"] == "collabAgentToolCall")
        .map(|frame| frame["params"]["item"]["status"].clone())
        .collect();
    assert_eq!(states, [json!("completed"), json!("inProgress")]);
    // Each child names itself by `agentPath`, as Codex's do (s4/f0244).
    let names: Vec<_> = frames
        .iter()
        .filter(|frame| frame["params"]["item"]["type"] == "subAgentActivity")
        .map(|frame| frame["params"]["item"]["agentPath"].clone())
        .collect();
    assert_eq!(names, [json!("Count math"), json!("Count readme")]);
    let interrupt = frames.iter().position(|frame| frame["id"] == 5).unwrap();
    assert!(
        frames[..interrupt]
            .iter()
            .all(|frame| frame["method"] != "turn/completed"),
        "the parent Turn stays live until Stop"
    );
}

#[test]
fn codex_worked_expand_case_interleaves_commentary_and_three_command_rows() {
    let frames = replay_native(
        "codex-motion-worked-expand",
        &codex_requests(&[json!({"id":5,"method":"thread/unsubscribe"})]),
    );
    let order: Vec<_> = frames
        .iter()
        .filter_map(|frame| match frame["method"].as_str() {
            Some("item/agentMessage/delta") => frame["params"]["itemId"].as_str(),
            Some("item/completed") => frame["params"]["item"]["id"].as_str(),
            _ => None,
        })
        .collect();
    assert_eq!(
        order,
        [
            "fixture-commentary-1",
            "fixture-tool-1",
            "fixture-commentary-2",
            "fixture-tool-2",
            "fixture-commentary-3",
            "fixture-tool-3",
            "fixture-answer",
        ]
    );
}

#[test]
fn pi_second_turn_cases_answer_a_bare_second_prompt() {
    // Pi discovers thinking levels once per activation; the second prompt
    // arrives alone and the replay must not wait for another discovery.
    for case in ["pi-annotation-sent", "pi-turn-navigator-hover"] {
        let frames = replay_native(
            case,
            &lines(&[
                json!({"type":"get_state","id":"pi-1"}),
                json!({"type":"get_available_models","id":"pi-2"}),
                json!({"type":"new_session","id":"pi-3"}),
                json!({"type":"prompt","id":"pi-4","message":"Inspect"}),
                json!({"type":"get_available_thinking_levels","id":"pi-5"}),
                json!({"type":"prompt","id":"pi-6","message":"Again"}),
            ]),
        );
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame["type"] == "agent_settled")
                .count(),
            2,
            "{case}"
        );
        assert!(
            frames
                .iter()
                .any(|frame| frame["id"] == "pi-6" && frame["success"] == true),
            "{case}"
        );
    }
}

#[test]
fn codex_name_hover_case_names_each_finished_child() {
    let frames = replay_native(
        "codex-subagents-name-hover",
        &codex_requests(&[json!({"id":5,"method":"thread/unsubscribe"})]),
    );
    let named: Vec<_> = frames
        .iter()
        .filter(|frame| frame["params"]["item"]["type"] == "subAgentActivity")
        .map(|frame| {
            let item = &frame["params"]["item"];
            (
                item["agentThreadId"].clone(),
                item["agentPath"].clone(),
                item["kind"].clone(),
            )
        })
        .collect();
    assert_eq!(
        named,
        [
            (
                json!("fixture-child-1"),
                json!("Count math"),
                json!("completed")
            ),
            (
                json!("fixture-child-2"),
                json!("Count readme"),
                json!("completed")
            ),
        ]
    );
}
