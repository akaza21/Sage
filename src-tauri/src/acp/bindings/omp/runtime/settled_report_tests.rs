use super::*;
use crate::acp::lifecycle::ToolState;

fn settled_decline(runtime: &mut OmpRuntime) {
    initialize(runtime, false);
    new_session(runtime);
    running(runtime);
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
}

fn later_update(status: Option<&str>, output: Option<&str>) -> Frame {
    let mut update = serde_json::Map::new();
    update.insert("sessionUpdate".to_owned(), json!("tool_call_update"));
    update.insert("toolCallId".to_owned(), json!("permission-tool"));
    if let Some(status) = status {
        update.insert("status".to_owned(), json!(status));
    }
    if let Some(output) = output {
        update.insert("content".to_owned(), json!([{"type":"text","text":output}]));
    }
    frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
        "sessionId":"omp-session-1","update":Value::Object(update)
    }}))
}

fn cards(events: &[AdapterEvent]) -> Vec<Option<ToolState>> {
    events
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Item {
                tool_state,
                payload: crate::acp::intent::NormalizedPayload::ToolCall { .. },
                ..
            } => Some(*tool_state),
            _ => None,
        })
        .collect()
}

#[test]
fn a_failed_report_after_a_committed_decline_is_absorbed_rather_than_rejected() {
    let mut runtime = fixture_runtime(41);
    settled_decline(&mut runtime);
    let batch = runtime
        .ingest(&later_update(Some("failed"), None), &Mapper)
        .expect("a post-decline terminal report is absorbed");
    assert_eq!(
        cards(batch.events()),
        vec![Some(ToolState::Declined)],
        "only the committed decline publishes a card"
    );
}

#[test]
fn a_second_differing_terminal_report_after_a_decline_conflicts() {
    let mut runtime = fixture_runtime(42);
    settled_decline(&mut runtime);
    runtime
        .ingest(&later_update(Some("failed"), None), &Mapper)
        .expect("first settled report");
    assert!(
        runtime
            .ingest(&later_update(Some("completed"), None), &Mapper)
            .is_err(),
        "a differing terminal report conflicts"
    );
}

#[test]
fn an_exact_duplicate_terminal_report_after_a_decline_is_a_no_op() {
    let mut runtime = fixture_runtime(43);
    settled_decline(&mut runtime);
    runtime
        .ingest(&later_update(Some("failed"), None), &Mapper)
        .expect("first settled report");
    let batch = runtime
        .ingest(&later_update(Some("failed"), None), &Mapper)
        .expect("an exact duplicate settled report is a no-op");
    assert!(
        cards(batch.events()).is_empty(),
        "a duplicate settled report republishes nothing"
    );
}

#[test]
fn a_nonterminal_report_after_a_committed_decline_is_ignored_late() {
    let mut runtime = fixture_runtime(44);
    settled_decline(&mut runtime);
    let batch = runtime
        .ingest(&later_update(Some("in_progress"), None), &Mapper)
        .expect("a nonterminal report after settlement is ignored late");
    assert_eq!(
        cards(batch.events()),
        vec![Some(ToolState::Declined)],
        "a nonterminal report never reopens the settled card"
    );
}

#[test]
fn a_same_state_declined_report_with_a_changed_payload_never_republishes_the_card() {
    let mut runtime = fixture_runtime(45);
    settled_decline(&mut runtime);
    let batch = runtime
        .ingest(
            &later_update(Some("declined"), Some("late output")),
            &Mapper,
        )
        .expect("a same-state report with a changed payload is absorbed");
    assert_eq!(
        cards(batch.events()),
        vec![Some(ToolState::Declined)],
        "the settled card is published exactly once"
    );
}
