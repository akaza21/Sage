//! Standard ACP web search classification from protocol fields only.

use serde_json::{Value, json};

use super::{
    profile::profile_for,
    translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator},
};
use crate::acp::intent::{
    NormalizedPayload, SafeResourceRef, ToolCategory, UnavailableResourceReason,
};

struct NoResources;

impl ResourceMapper for NoResources {
    fn map_absolute(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

fn tool(harness: &str, call: Value) -> (ToolCategory, Option<String>) {
    let mut translator =
        StandardAcpTranslator::new(profile_for(harness).unwrap(), "session-1").unwrap();
    translator.begin_prompt("prompt-1").unwrap();
    let events = translator
        .translate_notification_value(
            json!({"method": "session/update", "params": {"sessionId": "session-1", "update": call}}),
            &NoResources,
        )
        .unwrap();
    events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload:
                    NormalizedPayload::ToolCall {
                        category,
                        input_summary,
                        ..
                    },
                ..
            } => Some((
                *category,
                input_summary
                    .as_ref()
                    .map(|value| value.as_str().to_owned()),
            )),
            _ => None,
        })
        .expect("tool call")
}

#[test]
fn a_fetch_tool_whose_raw_input_is_a_query_is_a_web_search_for_every_profile() {
    for harness in ["claude-code", "opencode"] {
        let (category, query) = tool(
            harness,
            json!({"sessionUpdate": "tool_call", "toolCallId": "web-1", "kind": "fetch",
                   "title": "\"node test runner\"", "status": "in_progress",
                   "rawInput": {"query": "node test runner"}}),
        );
        assert_eq!(category, ToolCategory::WebSearch, "{harness}");
        assert_eq!(query.as_deref(), Some("node test runner"), "{harness}");
    }
}

#[test]
fn a_fetch_with_a_url_or_without_a_query_stays_a_fetch() {
    for raw in [
        json!({"url": "https://example.com", "query": "ignored"}),
        json!({"query": "  "}),
        json!({}),
    ] {
        let (category, _) = tool(
            "claude-code",
            json!({"sessionUpdate": "tool_call", "toolCallId": "fetch-1", "kind": "fetch",
                   "title": "Fetch", "status": "in_progress", "rawInput": raw}),
        );
        assert_eq!(category, ToolCategory::Fetch, "{raw}");
    }
}
