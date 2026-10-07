use super::*;
use crate::acp::lifecycle::ToolState;

#[test]
fn a_committed_decline_reaches_the_durable_card_before_the_turn_ends() {
    let mut runtime = fixture_runtime(31);
    initialize(&mut runtime, false);
    new_session(&mut runtime);
    let prompt = running(&mut runtime);
    let batch = runtime
        .ingest(&permission(61, "permission-tool"), &Mapper)
        .expect("permission");
    let token = batch.egress()[0].token().clone();
    runtime.commit_mandatory_egress(&token).expect("commit");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    let terminal = runtime
        .ingest(
            &response(&prompt, json!({"stopReason":"end_turn"})),
            &Mapper,
        )
        .expect("terminal");
    let events = terminal.events();
    let declined = events.iter().position(|event| {
        matches!(
            event,
            AdapterEvent::Item {
                tool_state: Some(ToolState::Declined),
                ..
            }
        )
    });
    let turn = events
        .iter()
        .position(|event| matches!(event, AdapterEvent::Turn { .. }));
    let declined = declined.expect("the declined tool card is published");
    let turn = turn.expect("the turn terminal is published");
    assert!(
        declined < turn,
        "the declined card is published before the turn ends"
    );
}

#[test]
fn a_declined_card_keeps_the_label_and_kind_the_permission_surface_showed() {
    let mut runtime = fixture_runtime(31);
    initialize(&mut runtime, false);
    new_session(&mut runtime);
    let prompt = running(&mut runtime);
    let batch = runtime
        .ingest(&permission(61, "permission-tool"), &Mapper)
        .expect("permission");
    let token = batch.egress()[0].token().clone();
    runtime.commit_mandatory_egress(&token).expect("commit");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    let terminal = runtime
        .ingest(
            &response(&prompt, json!({"stopReason":"end_turn"})),
            &Mapper,
        )
        .expect("terminal");
    let card = terminal
        .events()
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                tool_state: Some(ToolState::Declined),
                payload:
                    crate::acp::intent::NormalizedPayload::ToolCall {
                        label, category, ..
                    },
                ..
            } => Some((label.as_str().to_owned(), *category)),
            _ => None,
        })
        .expect("the declined tool card is published");
    assert_eq!(
        card.0, "Command tool",
        "the declined card keeps the label the permission surface showed"
    );
    assert_eq!(
        card.1,
        crate::acp::intent::ToolCategory::Execute,
        "the declined card keeps its kind"
    );
}

fn located_tool(runtime: &mut OmpRuntime, tool: &str) {
    runtime
        .ingest(
            &frame(json!({
                "jsonrpc":"2.0","method":"session/update","params":{
                    "sessionId":"omp-session-1","update":{
                        "sessionUpdate":"tool_call","toolCallId":tool,
                        "kind":"execute","status":"pending",
                        "locations":[{"path":"C:\\fixture\\src\\lib.rs"}]
                    }
                }
            })),
            &Mapper,
        )
        .expect("tool start");
}

#[test]
fn a_declined_card_keeps_the_locations_the_tool_reported() {
    let mut runtime = fixture_runtime(31);
    initialize(&mut runtime, false);
    new_session(&mut runtime);
    let prompt = running(&mut runtime);
    located_tool(&mut runtime, "permission-tool");
    let batch = runtime
        .ingest(&permission(61, "permission-tool"), &Mapper)
        .expect("permission");
    let token = batch.egress()[0].token().clone();
    runtime.commit_mandatory_egress(&token).expect("commit");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    let terminal = runtime
        .ingest(
            &response(&prompt, json!({"stopReason":"end_turn"})),
            &Mapper,
        )
        .expect("terminal");
    let located = terminal
        .events()
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                tool_state: Some(ToolState::Declined),
                payload: crate::acp::intent::NormalizedPayload::ToolCall { locations, .. },
                ..
            } => Some(locations.as_slice().len()),
            _ => None,
        })
        .expect("the declined tool card is published");
    assert_eq!(
        located, 1,
        "the declined card keeps the locations the tool reported"
    );
}
