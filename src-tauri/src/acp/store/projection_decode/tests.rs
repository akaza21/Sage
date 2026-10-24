use super::*;

#[test]
fn attempt_error_kind_survives_durable_decode_and_projection() {
    for kind in [None, Some("attempt")] {
        let mut wire = serde_json::json!({"kind":"error","payload":{
            "code":"Renamed label","message":"Attempt cause","retryable":false
        }});
        if let Some(kind) = kind {
            wire["payload"]["kind"] = serde_json::json!(kind);
        }
        let payload: NormalizedPayload = serde_json::from_value(wire).unwrap();
        let projected = content(
            payload,
            crate::acp::lifecycle::ItemLifecycle::Failed,
            None,
            None,
            None,
        )
        .unwrap();
        let encoded = serde_json::to_value(projected).unwrap();
        assert_eq!(
            encoded["payload"]
                .get("kind")
                .and_then(serde_json::Value::as_str),
            kind
        );
    }
}

#[test]
fn root_collaboration_item_without_activity_loads_from_history() {
    let payload = NormalizedPayload::Collaboration {
        label: SafeText::new("label", "Subagent").unwrap(),
        message: None,
        receiver_labels: BoundedVec::new("receiverLabels", vec![]).unwrap(),
        operation: Some(SafeText::new("operation", "started").unwrap()),
        agent_key: Some(SafeText::new("agentKey", "child-1").unwrap()),
        agent_keys: BoundedVec::new("agentKeys", vec![]).unwrap(),
        agent_states: BoundedVec::new("agentStates", vec![]).unwrap(),
    };
    assert!(
        content(
            payload,
            crate::acp::lifecycle::ItemLifecycle::Completed,
            None,
            None,
            None,
        )
        .is_ok()
    );
}

#[test]
fn activity_origin_collaboration_without_activity_is_rejected() {
    let payload = NormalizedPayload::Collaboration {
        label: SafeText::new("label", "Subagent").unwrap(),
        message: None,
        receiver_labels: BoundedVec::new("receiverLabels", vec![]).unwrap(),
        operation: Some(SafeText::new("operation", "started").unwrap()),
        agent_key: Some(SafeText::new("agentKey", "child-1").unwrap()),
        agent_keys: BoundedVec::new("agentKeys", vec![]).unwrap(),
        agent_states: BoundedVec::new("agentStates", vec![]).unwrap(),
    };
    let payload_json = serde_json::to_string(&payload).unwrap();
    let row = ItemRow {
        id: "item-1".into(),
        project_id: "project-a".into(),
        chat_id: "chat-a".into(),
        session_id: "session-1".into(),
        turn_id: "turn-1".into(),
        runtime_epoch: 1,
        ordinal: 0,
        origin: "harnessActivity".into(),
        activity_id: None,
        kind: "collaboration".into(),
        lifecycle: "completed".into(),
        tool_state: None,
        external_item_id: None,
        payload_bytes: payload_json.len() as i64,
        payload_digest: super::super::facts::hex_digest(payload_json.as_bytes()),
        payload_json,
        revision: 1,
        created_at: 1,
        updated_at: 1,
        terminal_at: Some(1),
    };
    assert!(matches!(
        decode_item(row, None, &[]),
        Err(StoreError::Corrupt("collaboration activity"))
    ));
}

#[test]
fn plan_projection_tracks_streaming_and_terminal_lifecycle() {
    let payload = NormalizedPayload::Plan {
        summary: SafeText::new("plan", "Inspect the project").unwrap(),
    };
    let streaming = content(
        payload.clone(),
        crate::acp::lifecycle::ItemLifecycle::Streaming,
        None,
        None,
        None,
    )
    .unwrap();
    let completed = content(
        payload,
        crate::acp::lifecycle::ItemLifecycle::Completed,
        None,
        None,
        None,
    )
    .unwrap();
    assert!(matches!(
        streaming,
        ItemContent::Plan { ref entries }
            if entries.as_slice()[0].state == PlanEntryState::InProgress
    ));
    assert!(matches!(
        completed,
        ItemContent::Plan { ref entries }
            if entries.as_slice()[0].state == PlanEntryState::Completed
    ));
}

#[test]
fn structured_plan_keeps_each_entry_state_and_compaction_is_not_a_tool() {
    use crate::acp::{
        contract::BoundedVec,
        intent::{NormalizedPlanEntry, NormalizedPlanState},
    };
    let plan = NormalizedPayload::PlanEntries {
        entries: BoundedVec::new(
            "planEntries",
            vec![
                NormalizedPlanEntry {
                    label: SafeText::new("planEntry", "Inspect").unwrap(),
                    state: NormalizedPlanState::Completed,
                },
                NormalizedPlanEntry {
                    label: SafeText::new("planEntry", "Patch").unwrap(),
                    state: NormalizedPlanState::InProgress,
                },
            ],
        )
        .unwrap(),
    };
    let projected = content(
        plan,
        crate::acp::lifecycle::ItemLifecycle::Streaming,
        None,
        None,
        None,
    )
    .unwrap();
    assert!(
        matches!(projected, ItemContent::Plan { entries } if entries.as_slice().iter().map(|entry| entry.state).collect::<Vec<_>>() == vec![PlanEntryState::Completed, PlanEntryState::InProgress])
    );
    assert!(matches!(
        content(
            NormalizedPayload::CompactionNotice,
            crate::acp::lifecycle::ItemLifecycle::Completed,
            None,
            None,
            None
        )
        .unwrap(),
        ItemContent::CompactionNotice {}
    ));
    assert_eq!(
        NormalizedPayload::CompactionNotice.kind(),
        crate::acp::lifecycle::ItemKind::CompactionNotice
    );
}

fn file_change(operation: crate::acp::intent::FileChangeOperation) -> ItemContent {
    let payload = NormalizedPayload::file_summary_only(
        crate::acp::intent::FileChangeFacts {
            resource: crate::acp::intent::SafeResourceLocation::Resource {
                resource: crate::acp::intent::SafeResourceRef::new(
                    crate::acp::contract::SageRootId::parse("root-one").unwrap(),
                    "src/lib.rs",
                )
                .unwrap(),
            },
            summary: SafeText::new("summary", "File changed").unwrap(),
            additions: Some(1),
            deletions: Some(0),
            operation,
        },
        false,
    );
    content(
        payload,
        crate::acp::lifecycle::ItemLifecycle::Completed,
        None,
        None,
        None,
    )
    .unwrap()
}

#[test]
fn a_created_payload_projects_the_created_operation_with_its_counts() {
    let projected = file_change(crate::acp::intent::FileChangeOperation::Created);
    let ItemContent::FileChangeDiff { changes, .. } = projected else {
        panic!("a file change payload projects a file change");
    };
    let change = &changes.as_slice()[0];
    assert_eq!(change.operation, FileOperation::Created);
    assert_eq!((change.additions, change.deletions), (Some(1), Some(0)));
}

#[test]
fn an_updated_payload_still_projects_the_updated_operation() {
    let projected = file_change(crate::acp::intent::FileChangeOperation::Updated);
    let ItemContent::FileChangeDiff { changes, .. } = projected else {
        panic!("a file change payload projects a file change");
    };
    assert_eq!(changes.as_slice()[0].operation, FileOperation::Updated);
}

#[test]
fn a_permission_projects_its_tool_category_and_only_a_harness_reason() {
    use crate::acp::{contract::SageServerRequestId, lifecycle::ServerRequestState};
    for reason in [Some("May I retry outside the sandbox?"), None] {
        let projected = content(
            NormalizedPayload::PermissionRequest {
                category: crate::acp::intent::ToolCategory::Execute,
                summary: SafeText::new("summary", "Run a command").unwrap(),
                offered_option_kinds: None,
                reason: reason.map(|value| SafeText::new("reason", value).unwrap()),
            },
            crate::acp::lifecycle::ItemLifecycle::Streaming,
            None,
            None,
            Some((
                SageServerRequestId::parse("string:approval-1").unwrap(),
                ServerRequestState::Received,
                PermissionRequestSource::Harness,
            )),
        )
        .unwrap();
        let wire = serde_json::to_value(&projected).unwrap();
        assert_eq!(wire["payload"]["category"], "execute");
        assert_eq!(wire["payload"]["label"], "Permission request");
        match reason {
            Some(reason) => assert_eq!(wire["payload"]["reason"], reason),
            None => assert!(wire["payload"].get("reason").is_none()),
        }
    }
}

#[test]
fn tool_kinds_project_mcp_server_identity_web_search_and_image_view() {
    use crate::acp::intent::{ToolCategory, ToolExecution};
    let tool = |category, execution| {
        let projected = content(
            NormalizedPayload::ToolCall {
                category,
                label: SafeText::new("label", "search_openai_docs").unwrap(),
                input_summary: None,
                output_summary: Some(SafeText::new("output", "{}").unwrap()),
                locations: Default::default(),
                command_activity: Default::default(),
                execution,
            },
            crate::acp::lifecycle::ItemLifecycle::Completed,
            None,
            None,
            None,
        )
        .unwrap();
        serde_json::to_value(&projected).unwrap()["payload"].clone()
    };
    let named = tool(
        ToolCategory::Mcp,
        ToolExecution {
            mcp_server: Some(Box::new(
                SafeText::new("server", "openaiDeveloperDocs").unwrap(),
            )),
            ..Default::default()
        },
    );
    assert_eq!(named["presentation"]["kind"], "mcp");
    assert_eq!(named["presentation"]["serverLabel"], "openaiDeveloperDocs");
    assert_eq!(named["presentation"]["toolLabel"], "search_openai_docs");
    assert_eq!(named["outputSummary"], "{}");
    let unnamed = tool(ToolCategory::Mcp, ToolExecution::default());
    assert_eq!(unnamed["presentation"]["serverLabel"], "MCP");
    assert_eq!(
        tool(ToolCategory::WebSearch, ToolExecution::default())["presentation"],
        serde_json::json!({"kind": "webSearch", "label": "search_openai_docs"})
    );
    let image = tool(ToolCategory::ImageView, ToolExecution::default());
    assert_eq!(image["presentation"]["kind"], "imageView");
    assert_eq!(image["presentation"]["resources"], serde_json::json!([]));
}
