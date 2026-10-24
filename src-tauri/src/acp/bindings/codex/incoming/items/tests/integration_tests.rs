use super::*;

fn tool_call(payload: NormalizedPayload) -> (ToolCategory, String, Option<String>, Option<String>) {
    let NormalizedPayload::ToolCall {
        category,
        label,
        input_summary,
        output_summary,
        ..
    } = payload
    else {
        panic!("expected a tool call");
    };
    (
        category,
        label.as_str().to_owned(),
        input_summary.map(|value| value.as_str().to_owned()),
        output_summary.map(|value| value.as_str().to_owned()),
    )
}

#[test]
fn mcp_structured_result_projects_as_pretty_json_output() {
    let call = item(json!({
        "type": "mcpToolCall", "id": "mcp-1", "status": "completed",
        "server": "openaiDeveloperDocs", "tool": "search_openai_docs",
        "arguments": {"query": "Responses API streaming"},
        "result": {
            "content": [{"type": "text", "text": "ignored when structured"}],
            "structuredContent": {"hits": [{"url": "https://developers.openai.com/api"}]}
        }
    }));
    let (payload, state, _) = normalize_item(&call, ItemLifecycle::Completed, &UnusedMapper)
        .expect("MCP call")
        .expect("item");
    assert_eq!(state, Some(ToolState::Completed));
    let (category, label, input, output) = tool_call(payload);
    assert_eq!(category, ToolCategory::Mcp);
    assert_eq!(label, "search_openai_docs");
    assert!(input.is_none(), "arguments are never shown");
    let output = output.expect("structured result");
    let parsed: Value = serde_json::from_str(&output).expect("still JSON");
    assert_eq!(
        parsed["hits"][0]["url"],
        "https://developers.openai.com/api"
    );
    assert!(output.contains('\n'), "pretty-printed");
}

#[test]
fn mcp_text_blocks_join_in_order_and_other_blocks_are_named_not_inlined() {
    let call = item(json!({
        "type": "mcpToolCall", "id": "mcp-2", "status": "completed",
        "server": "docs", "tool": "fetch",
        "arguments": {},
        "result": {"content": [
            {"type": "text", "text": "first"},
            {"type": "image", "data": "iVBORw0KGgo", "mimeType": "image/png"},
            {"type": "text", "text": "second"}
        ]}
    }));
    let (payload, _, _) = normalize_item(&call, ItemLifecycle::Completed, &UnusedMapper)
        .expect("MCP call")
        .expect("item");
    let (_, _, _, output) = tool_call(payload);
    assert_eq!(
        output.as_deref(),
        Some("first\n\n[image content]\n\nsecond")
    );
}

#[test]
fn mcp_failure_projects_only_the_harness_error_message() {
    let call = item(json!({
        "type": "mcpToolCall", "id": "mcp-3", "status": "failed",
        "server": "node_repl", "tool": "evaluate",
        "arguments": {"code": "throw new Error('sandbox probe')"},
        "error": {"message": "sandbox probe"}
    }));
    let (payload, state, lifecycle) =
        normalize_item(&call, ItemLifecycle::Completed, &UnusedMapper)
            .expect("MCP call")
            .expect("item");
    assert_eq!(state, Some(ToolState::Failed));
    assert_eq!(lifecycle, Some(ItemLifecycle::Failed));
    assert_eq!(tool_call(payload).3.as_deref(), Some("sandbox probe"));
}

#[test]
fn web_search_carries_its_query_and_settles_without_a_wire_status() {
    for (wire, expected) in [
        (
            json!({"query": "Node.js node:test documentation"}),
            "Node.js node:test documentation",
        ),
        (
            json!({"query": "", "action": {"type": "search", "query": "fallback query"}}),
            "fallback query",
        ),
        (
            json!({"query": "", "action": {"type": "openPage", "url": "https://nodejs.org/api/test.html"}}),
            "https://nodejs.org/api/test.html",
        ),
    ] {
        let mut value = json!({"type": "webSearch", "id": "web-1"});
        value
            .as_object_mut()
            .unwrap()
            .extend(wire.as_object().unwrap().clone());
        let search = item(value);
        let (started, started_state, _) =
            normalize_item(&search, ItemLifecycle::Started, &UnusedMapper)
                .expect("web search")
                .expect("item");
        assert_eq!(started_state, Some(ToolState::InProgress));
        let (category, label, query, output) = tool_call(started);
        assert_eq!(category, ToolCategory::WebSearch);
        assert_eq!(label, "Web search");
        assert_eq!(query.as_deref(), Some(expected));
        assert!(output.is_none());
        let (_, completed_state, _) =
            normalize_item(&search, ItemLifecycle::Completed, &UnusedMapper)
                .expect("web search")
                .expect("item");
        assert_eq!(completed_state, Some(ToolState::Completed));
    }
    let bare = item(json!({"type": "webSearch", "id": "web-2", "query": ""}));
    let (payload, _, _) = normalize_item(&bare, ItemLifecycle::Completed, &UnusedMapper)
        .expect("web search")
        .expect("item");
    assert!(tool_call(payload).2.is_none(), "no query is invented");
}

#[test]
fn reasoning_projects_its_summary_and_never_its_raw_content() {
    let both = item(json!({
        "type": "reasoning", "id": "reason-1",
        "summary": ["Checking the contract"], "content": ["raw private chain"]
    }));
    let (payload, _, _) = normalize_item(&both, ItemLifecycle::Completed, &UnusedMapper)
        .expect("reasoning")
        .expect("item");
    assert!(
        matches!(payload, NormalizedPayload::ThoughtSummary { summary }
        if summary.as_str() == "Checking the contract")
    );
    let content_only = item(json!({
        "type": "reasoning", "id": "reason-2", "summary": [], "content": ["raw"]
    }));
    assert!(
        normalize_item(&content_only, ItemLifecycle::Completed, &UnusedMapper)
            .expect("reasoning")
            .is_none()
    );
}

#[test]
fn command_actions_classify_reads_and_searches_without_the_raw_command() {
    for (actions, category, label) in [
        (
            json!([{"type": "read", "command": "cat package.json", "name": "package.json",
                    "path": "C:\\fixture\\package.json"}]),
            ToolCategory::Read,
            "Read package.json",
        ),
        (
            json!([{"type": "search", "command": "rg -n foo src", "query": "foo", "path": "src"}]),
            ToolCategory::Search,
            "Search foo in src",
        ),
        (
            json!([{"type": "unknown", "command": "node build.js"}]),
            ToolCategory::Execute,
            "rg -n foo src",
        ),
    ] {
        let command = item(json!({
            "type": "commandExecution", "id": "cmd-1", "status": "completed",
            "command": "rg -n foo src", "cwd": "C:\\fixture", "commandActions": actions,
            "aggregatedOutput": "src/a.ts:1:foo", "exitCode": 0
        }));
        let (payload, _, _) = normalize_item(&command, ItemLifecycle::Completed, &MappedResource)
            .expect("command")
            .expect("item");
        let (projected, projected_label, input, output) = tool_call(payload);
        assert_eq!(projected, category, "{actions}");
        assert_eq!(projected_label, label);
        assert_eq!(
            input.as_deref(),
            Some("rg -n foo src"),
            "the command stays in detail"
        );
        assert_eq!(output.as_deref(), Some("src/a.ts:1:foo"));
    }
}
