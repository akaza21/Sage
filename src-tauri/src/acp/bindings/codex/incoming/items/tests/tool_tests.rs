use super::*;
use crate::acp::bindings::codex::state::SubagentLabels;

#[test]
fn mcp_tool_call_names_only_bounded_safe_server_and_tool_fields() {
    for (server, tool_name, expected, expected_server) in [
        ("sage-browser", "openTab", "openTab", Some("sage-browser")),
        ("C:\\private\\server", "openTab", "openTab", None),
        ("sage-browser", "openTab\nsecret", "MCP tool", None),
        ("x", "a", "a", Some("x")),
    ] {
        let call = item(json!({
            "type": "mcpToolCall", "status": "completed",
            "server": server, "tool": tool_name,
            "arguments": {"token": "do-not-label"},
            "result": "do-not-label"
        }));
        let (payload, _, _) = normalize_item(&call, ItemLifecycle::Completed, &UnusedMapper)
            .expect("MCP call")
            .expect("item");
        let NormalizedPayload::ToolCall {
            label,
            execution,
            output_summary,
            ..
        } = payload
        else {
            panic!("MCP tool");
        };
        assert_eq!(label.as_str(), expected);
        assert_eq!(
            execution.mcp_server.as_ref().map(|server| server.as_str()),
            expected_server
        );
        assert!(output_summary.is_none(), "a non-object result is not shown");
    }
    let oversized = item(json!({
        "type": "mcpToolCall", "status": "completed",
        "server": "a".repeat(129), "tool": "openTab"
    }));
    let (payload, _, _) = normalize_item(&oversized, ItemLifecycle::Completed, &UnusedMapper)
        .expect("MCP call")
        .expect("item");
    assert!(
        matches!(payload, NormalizedPayload::ToolCall { label, execution, .. }
        if label.as_str() == "openTab" && execution.mcp_server.is_none()),
        "an unsafe server name is dropped, never shown"
    );
}

#[test]
fn schema_shaped_context_compaction_is_a_shared_notice() {
    let compacted = item(json!({
        "id": "compaction-1",
        "type": "contextCompaction"
    }));
    let (payload, state, terminal) =
        normalize_item(&compacted, ItemLifecycle::Completed, &UnusedMapper)
            .expect("context compaction")
            .expect("item");
    assert_eq!(state, None);
    assert_eq!(terminal, Some(ItemLifecycle::Completed));
    assert!(matches!(payload, NormalizedPayload::CompactionNotice));
}

#[test]
fn context_compaction_ignores_undeclared_status_and_remains_completed() {
    let compacted = item(json!({
        "id": "compaction-1",
        "type": "contextCompaction",
        "status": "failed"
    }));
    let (_, state, terminal) = normalize_item(&compacted, ItemLifecycle::Completed, &UnusedMapper)
        .expect("context compaction")
        .expect("item");
    assert_eq!(state, None);
    assert_eq!(terminal, Some(ItemLifecycle::Completed));
}

#[test]
fn a_seen_safe_agent_path_names_a_later_grouped_completion() {
    let activity = item(json!({
        "type": "subAgentActivity", "id": "activity-1",
        "agentThreadId": "child-1", "agentPath": "Code review", "kind": "started"
    }));
    let wait = item(json!({
        "type": "collabAgentToolCall", "id": "wait-1", "tool": "wait",
        "receiverThreadIds": ["child-1"],
        "agentsStates": {"child-1": {"status": "completed"}}
    }));
    let mut labels = SubagentLabels::default();
    let (mut activity, _, _) = normalize_item(&activity, ItemLifecycle::Completed, &UnusedMapper)
        .unwrap()
        .unwrap();
    labels.apply(&mut activity);
    let (mut completion, _, _) = normalize_item(&wait, ItemLifecycle::Completed, &UnusedMapper)
        .unwrap()
        .unwrap();
    labels.apply(&mut completion);
    assert!(
        matches!(completion, NormalizedPayload::Collaboration { receiver_labels, .. }
        if receiver_labels.as_slice()[0].as_str() == "Code review")
    );
}
#[test]
fn subagent_labels_retain_300_identities_and_evict_only_at_301() {
    let mut labels = SubagentLabels::default();
    for index in 0..300 {
        let activity = item(json!({
            "type": "subAgentActivity", "id": format!("activity-{index}"),
            "agentThreadId": format!("child-{index}"), "agentPath": format!("Reviewer {index}"),
            "kind": "started"
        }));
        let (mut payload, _, _) =
            normalize_item(&activity, ItemLifecycle::Completed, &UnusedMapper)
                .unwrap()
                .unwrap();
        labels.apply(&mut payload);
    }
    for (expected, extra) in [("Reviewer 0", false), ("", true)] {
        if extra {
            let activity = item(json!({"type": "subAgentActivity", "id": "activity-300",
                "agentThreadId": "child-300", "agentPath": "Reviewer 300", "kind": "started"}));
            let (mut payload, _, _) =
                normalize_item(&activity, ItemLifecycle::Completed, &UnusedMapper)
                    .unwrap()
                    .unwrap();
            labels.apply(&mut payload);
        }
        let wait = item(
            json!({"type": "collabAgentToolCall", "id": "wait-1", "tool": "wait",
            "receiverThreadIds": ["child-0", "unnamed"],
            "agentsStates": {"child-0": {"status": "completed"}, "unnamed": {"status": "completed"}}}),
        );
        let (mut payload, _, _) = normalize_item(&wait, ItemLifecycle::Completed, &UnusedMapper)
            .unwrap()
            .unwrap();
        labels.apply(&mut payload);
        assert!(
            matches!(payload, NormalizedPayload::Collaboration { receiver_labels, .. }
            if receiver_labels.as_slice().iter().map(SafeText::as_str).collect::<Vec<_>>() == vec![expected, ""])
        );
    }
}

#[test]
fn a_later_unlabelled_child_activity_retains_its_supplied_identity() {
    let mut labels = SubagentLabels::default();
    for (path, kind) in [(Some("Reviewer"), "started"), (None, "interrupted")] {
        let activity = item(json!({"type": "subAgentActivity", "id": kind,
            "agentThreadId": "child-1", "agentPath": path, "kind": kind}));
        let (mut payload, _, _) =
            normalize_item(&activity, ItemLifecycle::Completed, &UnusedMapper)
                .unwrap()
                .unwrap();
        labels.apply(&mut payload);
        assert!(
            matches!(payload, NormalizedPayload::Collaboration { label, .. } if label.as_str() == "Reviewer")
        );
    }
}

#[test]
fn codex_spawn_preserves_the_verified_operation_and_receivers() {
    let spawn = item(json!({
        "type": "collabAgentToolCall",
        "id": "spawn-1",
        "tool": "spawnAgent",
        "status": "completed",
        "senderThreadId": "parent",
        "receiverThreadIds": ["child-1"],
        "agentsStates": {"child-1": {"status": "running"}}
    }));
    let (payload, _, _) = normalize_item(&spawn, ItemLifecycle::Completed, &UnusedMapper)
        .expect("spawn")
        .expect("item");
    assert!(
        matches!(payload, NormalizedPayload::Collaboration { label, operation: Some(operation), agent_keys, agent_states, .. }
            if label.as_str() == "spawnAgent"
                && operation.as_str() == "spawnAgent"
                && agent_keys.as_slice()[0].as_str() == "child-1"
                && agent_states.as_slice()[0].1.as_str() == "running")
    );
}

#[test]
fn codex_message_and_roster_tools_keep_their_operation_label() {
    for tool in [
        "sendMessage",
        "followupTask",
        "interruptAgent",
        "listAgents",
    ] {
        let call = item(json!({
            "type": "collabAgentToolCall", "id": tool, "tool": tool, "status": "completed",
            "senderThreadId": "parent", "receiverThreadIds": [], "agentsStates": {}
        }));
        let (payload, _, _) = normalize_item(&call, ItemLifecycle::Completed, &UnusedMapper)
            .unwrap()
            .unwrap();
        assert!(
            matches!(payload, NormalizedPayload::Collaboration { label, operation: Some(operation), .. }
                if label.as_str() == tool && operation.as_str() == tool),
            "{tool}"
        );
    }
}

#[test]
fn completed_child_activity_preserves_its_emitted_operation() {
    let activity = item(json!({"type": "subAgentActivity", "id": "done-1",
        "agentThreadId": "child-1", "agentPath": "Reviewer", "kind": "completed"}));
    let (payload, _, _) = normalize_item(&activity, ItemLifecycle::Completed, &UnusedMapper)
        .unwrap()
        .unwrap();
    assert!(
        matches!(payload, NormalizedPayload::Collaboration { operation: Some(operation), .. }
        if operation.as_str() == "completed")
    );
}

#[test]
fn subagent_activity_uses_only_safe_display_paths() {
    for (agent_path, expected) in [
        ("Code review", "Code review"),
        ("/root/private", "Subagent"),
    ] {
        let activity = item(json!({
            "type": "subAgentActivity",
            "id": "activity-1",
            "agentThreadId": "private-thread",
            "agentPath": agent_path,
            "kind": "started"
        }));
        let (payload, _, _) = normalize_item(&activity, ItemLifecycle::Completed, &UnusedMapper)
            .expect("activity")
            .expect("item");
        assert!(
            matches!(payload, NormalizedPayload::Collaboration { label, agent_key: Some(key), .. }
                if label.as_str() == expected && key.as_str() == "private-thread")
        );
    }
}

#[test]
fn dynamic_browse_keeps_its_curated_label_and_admitted_url() {
    let item = item(json!({
        "type": "dynamicToolCall",
        "id": "browse-1",
        "tool": "sage_browse",
        "arguments": {"url": "http://127.0.0.1:43124/proof"},
        "status": "inProgress"
    }));
    let (payload, state, terminal) = normalize_item(&item, ItemLifecycle::Started, &UnusedMapper)
        .unwrap()
        .expect("admitted browse");
    assert_eq!(state, Some(ToolState::InProgress));
    assert_eq!(terminal, None);
    assert!(matches!(
        payload,
        NormalizedPayload::ToolCall { category: ToolCategory::Fetch, label, input_summary: Some(summary), .. }
            if label.as_str() == "_sage/browse" && summary.as_str() == "http://127.0.0.1:43124/proof"
    ));
}

#[test]
fn command_execution_uses_only_typed_output_and_a_single_read_action() {
    let command = item(json!({
        "type": "commandExecution",
        "status": "completed",
        "aggregatedOutput": "Bearer sk-codex-secret\nsecond\n",
        "command": "cat C:\\private\\secret.txt",
        "commandActions": [{
            "type": "read",
            "command": "cat package.json",
            "name": "read package",
            "path": "C:\\fixture\\package.json"
        }]
    }));
    let (started, state, lifecycle) =
        normalize_item(&command, ItemLifecycle::Started, &MappedResource)
            .expect("typed command")
            .expect("item");
    assert_eq!(state, Some(ToolState::Completed));
    assert_eq!(lifecycle, Some(ItemLifecycle::Completed));
    assert!(matches!(
        started,
        NormalizedPayload::ToolCall {
            category: ToolCategory::Read,
            label,
            input_summary: None,
            output_summary: Some(output),
            ..
        } if output.as_str() == "Bearer [redacted]\nsecond\n"
            && label.as_str() == "Read package.json"
    ));

    let (payload, state, lifecycle) =
        normalize_item(&command, ItemLifecycle::Completed, &MappedResource)
            .expect("typed command")
            .expect("item");
    assert_eq!(state, Some(ToolState::Completed));
    assert_eq!(lifecycle, Some(ItemLifecycle::Completed));
    let NormalizedPayload::ToolCall {
        category,
        output_summary,
        locations,
        ..
    } = payload
    else {
        panic!("expected command update");
    };
    assert_eq!(category, ToolCategory::Read, "a typed read is a read");
    assert_eq!(
        output_summary.expect("output").as_str(),
        "Bearer [redacted]\nsecond\n"
    );
    assert!(
        matches!(locations.as_slice(), [crate::acp::intent::SafeResourceLocation::Resource { resource }]
            if resource.relative_path.as_str() == "package.json")
    );
}

#[test]
fn command_execution_never_derives_a_location_from_nonread_or_multiple_actions() {
    for actions in [
        json!([]),
        json!({"type":"read","path":"C:\\fixture\\package.json"}),
        json!([{"type":"read"}]),
        json!([{"type":"read","path":""}]),
        json!([{"type":"write","path":"C:\\fixture\\package.json"}]),
        json!([
            {"type":"read","path":"C:\\fixture\\package.json"},
            {"type":"read","path":"C:\\fixture\\other.json"}
        ]),
    ] {
        let command = item(json!({
            "type": "commandExecution", "status": "completed",
            "aggregatedOutput": "safe", "commandActions": actions
        }));
        let (payload, _, _) = normalize_item(&command, ItemLifecycle::Completed, &MappedResource)
            .expect("command")
            .expect("item");
        assert!(
            matches!(payload, NormalizedPayload::ToolCall { locations, .. }
                if locations.as_slice().is_empty())
        );
    }
}

#[test]
fn file_change_maps_to_typed_unavailable_without_invented_pair_text() {
    for (name, changes) in [
        (
            "present",
            json!([{"path": "C:\\fixture\\package.json", "kind": {"type": "update"}, "diff": "opaque"}]),
        ),
        (
            "missing-diff",
            json!([{"path": "C:\\fixture\\package.json", "kind": {"type": "update"}}]),
        ),
        (
            "oversized-diff",
            json!([{"path": "C:\\fixture\\package.json", "kind": {"type": "update"}, "diff": "x".repeat(65_536)}]),
        ),
    ] {
        let file = item(json!({
            "type": "fileChange",
            "id": format!("file-{name}"),
            "status": "completed",
            "changes": changes,
        }));
        let (payload, state, lifecycle) =
            normalize_item(&file, ItemLifecycle::Completed, &MappedResource)
                .expect("codex fileChange")
                .expect("item");
        assert_eq!(
            state, None,
            "{name} must not attach tool state to a file diff"
        );
        assert_eq!(lifecycle, None, "{name} follows its notification lifecycle");
        assert!(
            matches!(
                payload,
                NormalizedPayload::FileChangeDiff {
                    pair_available: false,
                    snapshot_revision: 0,
                    old_bytes: 0,
                    new_bytes: 0,
                    ..
                }
            ),
            "{name} must remain summary-only without a retained pair"
        );
        assert!(
            !format!("{payload:?}").contains("opaque"),
            "{name} must not retain wire diff text"
        );
    }
    let malformed = item(json!({
        "type": "fileChange",
        "id": "file-malformed-path",
        "status": "completed",
        "changes": json!([{"path": "", "kind": {"type": "update"}, "diff": "opaque"}]),
    }));
    assert!(
        normalize_item(&malformed, ItemLifecycle::Completed, &MappedResource).is_err(),
        "empty path must fail closed as malformed"
    );
}

#[test]
fn file_change_multi_resource_is_typed_unsupported() {
    let file = item(json!({
        "type": "fileChange",
        "id": "file-multi",
        "status": "completed",
        "changes": [
            {"path": "C:\\fixture\\package.json", "kind": {"type": "update"}, "diff": "a"},
            {"path": "C:\\fixture\\other.json", "kind": {"type": "update"}, "diff": "b"},
        ],
    }));
    let (payload, _, lifecycle) = normalize_item(&file, ItemLifecycle::Completed, &MappedResource)
        .expect("multi")
        .expect("item");
    assert_eq!(lifecycle, Some(ItemLifecycle::Unsupported));
    assert!(matches!(payload, NormalizedPayload::Unsupported { .. }));
}

#[test]
fn spawn_keeps_only_the_wire_supplied_bounded_model_and_effort() {
    let spawn = |model: serde_json::Value, effort: serde_json::Value| {
        let call = item(json!({
            "type": "collabAgentToolCall", "id": "spawn-1", "tool": "spawnAgent",
            "status": "completed", "senderThreadId": "parent",
            "receiverThreadIds": ["child-1"],
            "agentsStates": {"child-1": {"status": "running"}},
            "model": model, "reasoningEffort": effort
        }));
        let (payload, _, _) = normalize_item(&call, ItemLifecycle::Completed, &UnusedMapper)
            .unwrap()
            .unwrap();
        let NormalizedPayload::Collaboration {
            model,
            reasoning_effort,
            ..
        } = payload
        else {
            panic!("spawn collaboration");
        };
        (
            model.map(|value| value.as_str().to_owned()),
            reasoning_effort.map(|value| value.as_str().to_owned()),
        )
    };
    assert_eq!(
        spawn(json!("gpt-6-astra"), json!("medium")),
        (Some("gpt-6-astra".into()), Some("medium".into()))
    );
    assert_eq!(spawn(json!(null), json!(null)), (None, None));
    assert_eq!(
        spawn(json!("bad\nmodel"), json!("x".repeat(33))),
        (None, None)
    );
    assert_eq!(spawn(json!("m".repeat(257)), json!("")), (None, None));
    let activity = item(json!({"type": "subAgentActivity", "id": "a-1",
        "agentThreadId": "child-1", "kind": "started", "model": "gpt-6-astra"}));
    let (payload, _, _) = normalize_item(&activity, ItemLifecycle::Completed, &UnusedMapper)
        .unwrap()
        .unwrap();
    assert!(matches!(
        payload,
        NormalizedPayload::Collaboration { model: None, .. }
    ));
}
