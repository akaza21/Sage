use serde_json::{Value, json};

use super::{
    profile::profile_for,
    translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator},
};
use crate::acp::{
    intent::{SafeResourceRef, UnavailableResourceReason},
    lifecycle::ItemLifecycle,
};

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

fn chunk(
    translator: &mut StandardAcpTranslator,
    kind: &str,
    text: &str,
) -> Vec<(String, ItemLifecycle)> {
    let update = json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}});
    send(translator, update)
}

fn send(translator: &mut StandardAcpTranslator, update: Value) -> Vec<(String, ItemLifecycle)> {
    items(
        translator
            .translate_notification_value(
                json!({"method":"session/update","params":{"sessionId":"session-1","update":update}}),
                &NoResources,
            )
            .unwrap(),
    )
}

fn finish(translator: &mut StandardAcpTranslator, result: Value) -> Vec<(String, ItemLifecycle)> {
    let mut frame = json!({"jsonrpc":"2.0","id":7});
    frame
        .as_object_mut()
        .unwrap()
        .extend(result.as_object().unwrap().clone());
    items(translator.translate_prompt_result_value(frame).unwrap())
}

fn items(events: Vec<AdapterEvent>) -> Vec<(String, ItemLifecycle)> {
    events
        .into_iter()
        .filter_map(|event| match event {
            AdapterEvent::Item {
                external_key,
                lifecycle,
                ..
            } => Some((external_key.as_str().to_owned(), lifecycle)),
            _ => None,
        })
        .collect()
}

fn closed(keys: &[&str]) -> Vec<(String, ItemLifecycle)> {
    keys.iter()
        .map(|key| ((*key).to_owned(), ItemLifecycle::Completed))
        .collect()
}

#[test]
fn answer_text_ends_the_thought_block_and_a_later_thought_opens_a_new_item() {
    let mut translator = opened();
    assert_eq!(
        chunk(&mut translator, "agent_thought_chunk", "Plan."),
        [("thought:7".to_owned(), ItemLifecycle::Streaming)]
    );
    assert_eq!(
        chunk(&mut translator, "agent_message_chunk", "Done"),
        [
            ("thought:7".to_owned(), ItemLifecycle::Completed),
            ("agent-message:7".to_owned(), ItemLifecycle::Streaming)
        ]
    );
    assert_eq!(
        chunk(&mut translator, "agent_message_chunk", "."),
        [("agent-message:7".to_owned(), ItemLifecycle::Streaming)]
    );
    assert_eq!(
        chunk(&mut translator, "agent_thought_chunk", "Check."),
        [("thought-1:7".to_owned(), ItemLifecycle::Streaming)]
    );
    assert_eq!(
        finish(&mut translator, json!({"result":{"stopReason":"end_turn"}})),
        closed(&["agent-message:7", "thought-1:7"])
    );
}

#[test]
fn every_delivered_stop_reason_closes_text_thought_and_plan() {
    for stop in ["end_turn", "max_tokens", "max_turn_requests", "refusal"] {
        let mut translator = opened();
        chunk(&mut translator, "agent_thought_chunk", "Plan.");
        send(
            &mut translator,
            json!({"sessionUpdate":"plan","entries":[{"content":"Edit","priority":"high","status":"pending"}]}),
        );
        let completed: Vec<_> = finish(&mut translator, json!({"result":{"stopReason":stop}}))
            .into_iter()
            .filter(|(_, lifecycle)| *lifecycle == ItemLifecycle::Completed)
            .collect();
        assert_eq!(completed, closed(&["plan:7", "thought:7"]), "{stop}");
    }
}

#[test]
fn a_cancelled_or_errored_prompt_leaves_open_output_to_the_turn_sweep() {
    for result in [
        json!({"result":{"stopReason":"cancelled"}}),
        json!({"error":{"code":-32603,"message":"backend down"}}),
    ] {
        let mut translator = opened();
        chunk(&mut translator, "agent_message_chunk", "Partial");
        let emitted = finish(&mut translator, result.clone());
        assert!(
            emitted.iter().all(|(key, _)| key != "agent-message:7"),
            "{result}: {emitted:?}"
        );
    }
}
