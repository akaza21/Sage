use serde_json::{Value, json};

use super::{
    profile::profile_for,
    translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator},
};
use sage_acp_protocol::intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason};

struct NoResources;

impl ResourceMapper for NoResources {
    fn map_absolute(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

fn translator() -> StandardAcpTranslator {
    StandardAcpTranslator::new(profile_for("gemini-cli").unwrap(), "session-1").unwrap()
}

fn update(translator: &mut StandardAcpTranslator, update: Value) -> Vec<AdapterEvent> {
    translator
        .translate_notification_value(
            json!({"method":"session/update","params":{"sessionId":"session-1","update":update}}),
            &NoResources,
        )
        .unwrap()
}

fn finish(translator: &mut StandardAcpTranslator, usage: Value) -> Vec<AdapterEvent> {
    translator
        .translate_prompt_result_value(
            json!({"jsonrpc":"2.0","id":"7","result":{"stopReason":"end_turn","usage":usage}}),
        )
        .unwrap()
}

fn usage(events: &[AdapterEvent]) -> Vec<(u64, u64, Option<u32>)> {
    events
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Usage(NormalizedPayload::Usage {
                input_tokens,
                output_tokens,
                context_window,
            }) => Some((*input_tokens, *output_tokens, *context_window)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_stated_context_is_the_turns_snapshot_and_the_prompts_spend_is_not() {
    let mut translator = translator();
    translator.begin_prompt("7").unwrap();
    let stated = update(
        &mut translator,
        json!({"sessionUpdate":"usage_update","used":23_753,"size":1_000_000}),
    );
    assert_eq!(usage(&stated), [(23_753, 0, Some(1_000_000))]);
    let terminal = finish(
        &mut translator,
        json!({"inputTokens":2,"outputTokens":4,"cachedReadTokens":23_747,"totalTokens":23_753}),
    );
    assert!(usage(&terminal).is_empty());
}

#[test]
fn without_a_stated_context_a_prompts_summed_spend_is_not_taken_for_one() {
    let mut translator = translator();
    translator.begin_prompt("7").unwrap();
    let terminal = finish(
        &mut translator,
        json!({"inputTokens":16,"outputTokens":4,"cachedReadTokens":800_000,"cachedWriteTokens":10}),
    );
    assert!(usage(&terminal).is_empty());
}

#[test]
fn a_size_stated_outside_a_turn_reaches_the_next_turns_snapshot() {
    let mut translator = translator();
    let idle = update(
        &mut translator,
        json!({"sessionUpdate":"usage_update","used":500,"size":200_000}),
    );
    assert!(usage(&idle).is_empty());
    translator.begin_prompt("7").unwrap();
    let stated = update(
        &mut translator,
        json!({"sessionUpdate":"usage_update","used":9_000}),
    );
    assert_eq!(usage(&stated), [(9_000, 0, Some(200_000))]);
}
