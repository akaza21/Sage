use serde_json::{Value, json};

use super::{
    profile::profile_for,
    translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator, TranslationError},
};
use crate::acp::{
    intent::{NormalizedPayload, SafeResourceRef, ToolExecution, UnavailableResourceReason},
    lifecycle::ItemLifecycle,
};
use sage_core::limits::LIMITS;

struct NoResources;

impl ResourceMapper for NoResources {
    fn map_absolute(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

fn opened() -> StandardAcpTranslator {
    let mut translator =
        StandardAcpTranslator::new(profile_for("gemini-cli").unwrap(), "session-1").unwrap();
    translator.begin_prompt("7").unwrap();
    translator
}

fn send(
    translator: &mut StandardAcpTranslator,
    update: Value,
) -> Result<Vec<AdapterEvent>, TranslationError> {
    translator.translate_notification_value(
        json!({"method":"session/update","params":{"sessionId":"session-1","update":update}}),
        &NoResources,
    )
}

fn tool_row(events: &[AdapterEvent]) -> (&str, &ToolExecution) {
    events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload:
                    NormalizedPayload::ToolCall {
                        label, execution, ..
                    },
                ..
            } => Some((label.as_str(), execution)),
            _ => None,
        })
        .expect("tool row")
}

fn run_command(translator: &mut StandardAcpTranslator, extra: Value) -> Vec<AdapterEvent> {
    let mut call = json!({"sessionUpdate":"tool_call","toolCallId":"run","kind":"execute",
        "status":"in_progress"});
    call.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    send(translator, call).unwrap()
}

#[test]
fn an_update_that_only_reports_an_exit_code_is_not_a_duplicate() {
    let mut translator = opened();
    run_command(&mut translator, json!({"title":"make"}));
    let exit_only = json!({"sessionUpdate":"tool_call_update","toolCallId":"run",
        "rawOutput":{"exitCode":2}});
    let reported = send(&mut translator, exit_only.clone()).unwrap();
    assert_eq!(tool_row(&reported).1.exit_code, Some(2));
    assert!(matches!(
        send(&mut translator, exit_only).unwrap().as_slice(),
        [AdapterEvent::IgnoredDuplicate]
    ));
}

#[test]
fn a_command_line_reported_later_replaces_the_untitled_fallback_label() {
    let mut translator = opened();
    assert_eq!(
        tool_row(&run_command(&mut translator, json!({}))).0,
        "Command tool"
    );
    let named = send(
        &mut translator,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"run",
            "rawInput":{"command":"make check"}}),
    )
    .unwrap();
    assert_eq!(tool_row(&named).0, "make check");

    let mut titled = opened();
    run_command(&mut titled, json!({"title":"Build"}));
    let kept = send(
        &mut titled,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"run",
            "rawInput":{"command":"make check"}}),
    )
    .unwrap();
    assert_eq!(tool_row(&kept).0, "Build");
}

#[test]
fn terminal_output_and_exit_code_survive_a_status_only_final_frame() {
    let mut translator = opened();
    let started = run_command(
        &mut translator,
        json!({"title":"make","rawOutput":{"exitCode":1},
            "content":[{"type":"terminal","terminalId":"term-1","output":"error: boom"}]}),
    );
    assert_eq!(
        tool_row(&started)
            .1
            .output_tail
            .as_ref()
            .map(|tail| tail.as_str()),
        Some("error: boom")
    );
    let settled = send(
        &mut translator,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"run","status":"failed"}),
    )
    .unwrap();
    let (_, execution) = tool_row(&settled);
    assert_eq!(execution.exit_code, Some(1));
    assert_eq!(
        execution.output_tail.as_ref().map(|tail| tail.as_str()),
        Some("error: boom")
    );
    assert!(!execution.output_truncated);
}

#[test]
fn a_diff_too_different_to_count_reports_unknown_line_counts() {
    let old: String = (0..2_000).map(|line| format!("old {line}\n")).collect();
    let new: String = (0..2_000).map(|line| format!("new {line}\n")).collect();
    let mut translator = opened();
    let events = send(
        &mut translator,
        json!({"sessionUpdate":"tool_call","toolCallId":"edit","kind":"edit","status":"pending",
            "content":[{"type":"diff","path":"/elsewhere/a.txt","oldText":old,"newText":new}]}),
    )
    .unwrap();
    let counts = events.iter().find_map(|event| match event {
        AdapterEvent::Item {
            payload:
                NormalizedPayload::FileChangeDiff {
                    additions,
                    deletions,
                    ..
                },
            ..
        } => Some((*additions, *deletions)),
        _ => None,
    });
    assert_eq!(counts, Some((None, None)));
}

#[test]
fn plans_with_distinct_ids_count_toward_the_turn_item_cap() {
    let mut translator = opened();
    let plan = |id: usize| {
        json!({"sessionUpdate":"plan","itemId":format!("plan-{id}"),
            "entries":[{"content":"Edit","priority":"high","status":"pending"}]})
    };
    let cap = LIMITS.wire.items_per_turn as usize;
    for id in 0..cap {
        send(&mut translator, plan(id)).unwrap();
    }
    assert!(matches!(
        send(&mut translator, plan(cap)),
        Err(TranslationError::ItemLimit)
    ));
    send(&mut translator, plan(0)).expect("an admitted plan may still be replaced");
}

#[test]
fn a_refused_answer_chunk_leaves_the_thought_open_for_the_next_one() {
    let mut translator = opened();
    send(
        &mut translator,
        json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Plan."}}),
    )
    .unwrap();
    assert!(
        send(
            &mut translator,
            json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text"}}),
        )
        .is_err()
    );
    let answered = send(
        &mut translator,
        json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done"}}),
    )
    .unwrap();
    let lifecycles: Vec<_> = answered
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Item {
                external_key,
                lifecycle,
                ..
            } => Some((external_key.as_str().to_owned(), *lifecycle)),
            _ => None,
        })
        .collect();
    assert_eq!(
        lifecycles,
        [
            ("thought:7".to_owned(), ItemLifecycle::Completed),
            ("agent-message:7".to_owned(), ItemLifecycle::Streaming)
        ]
    );
}
