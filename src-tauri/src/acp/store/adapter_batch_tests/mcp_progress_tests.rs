use super::*;
use crate::acp::{intent::ToolExecution, lifecycle::ToolState, projection::ToolPresentation};

fn mcp_item(owner: &AdapterStoreOwner, progress: &str, now_millis: i64) -> AdapterInboundEvent {
    let payload = NormalizedPayload::ToolCall {
        category: ToolCategory::Mcp,
        label: SafeText::new("label", "search").unwrap(),
        input_summary: None,
        output_summary: None,
        locations: Default::default(),
        command_activity: Default::default(),
        execution: ToolExecution {
            progress: Some(Box::new(SafeText::new("progress", progress).unwrap())),
            ..Default::default()
        },
    };
    let json = serde_json::to_string(&payload).unwrap();
    AdapterInboundEvent::Item(ItemIngressCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: owner.turn_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        external_item_id: ExternalId::new("external", "mcp-1").unwrap(),
        activity_id: None,
        payload,
        payload_digest: facts::hex_digest(json.as_bytes()),
        lifecycle: ItemLifecycle::Started,
        tool_state: Some(ToolState::InProgress),
        now_millis,
    })
}

fn projected_progress(fixture: &Fixture) -> Option<String> {
    let page = load_chat_page(
        &fixture.connection,
        fixture.owner.project_id.clone(),
        fixture.owner.chat_id.clone(),
        None,
    )
    .unwrap();
    let item = page.turns.as_slice()[0]
        .items
        .as_slice()
        .iter()
        .find(|item| matches!(item.content, ItemContent::ToolCall { .. }))
        .expect("the MCP call is projected");
    let ItemContent::ToolCall {
        presentation: ToolPresentation::Mcp { progress, .. },
        ..
    } = &item.content
    else {
        panic!("expected an MCP presentation, got {:?}", item.content)
    };
    progress.as_ref().map(|line| line.as_str().to_owned())
}

#[test]
fn a_running_mcp_line_is_projected_until_the_store_terminalizes_the_call() {
    let mut fixture = fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                AdapterInboundEvent::Turn {
                    state: TurnState::Running,
                    now_millis: 6,
                },
                mcp_item(&fixture.owner, "Indexed 2 of 3 pages", 7),
            ],
        ),
    )
    .unwrap();
    assert_eq!(
        projected_progress(&fixture).as_deref(),
        Some("Indexed 2 of 3 pages")
    );

    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Interrupted,
                now_millis: 9,
            }],
        ),
    )
    .unwrap();
    assert_eq!(
        projected_progress(&fixture),
        None,
        "a call the store cancelled keeps no running status line"
    );
}
