use super::*;

#[test]
fn lost_steer_fixture_answers_goal_hydration_without_ending_the_session() {
    let case = crate::acp_protocol::native_cases::JOURNEY_CASES
        .iter()
        .find(|case| case.id == "codex-steer-lost-result")
        .unwrap();
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let request = br#"{"jsonrpc":"2.0","id":4,"method":"thread/goal/get","params":{}}
"#;
    let mut input = std::io::Cursor::new(request);
    let mut output = Vec::new();
    let mut error = Vec::new();
    let mut state = super::super::WireState::new();
    super::super::execute_with_io(&steps[4], &mut state, &mut input, &mut output, &mut error)
        .unwrap();
    super::super::execute_with_io(&steps[5], &mut state, &mut input, &mut output, &mut error)
        .unwrap();
    assert_eq!(input.position(), request.len() as u64);
    assert_eq!(
        serde_json::from_slice::<Value>(&output).unwrap(),
        json!({"jsonrpc":"2.0","id":4,"error":{"code":-32601,"message":"Method not found"}})
    );
    assert!(error.is_empty());
    assert_eq!(
        steps[6],
        json!({"op":"awaitClient","kind":"codexTurnStart"})
    );
}

#[test]
fn lost_steer_result_consumes_the_correlated_write_without_emitting_a_reply() {
    let case = crate::acp_protocol::native_cases::JOURNEY_CASES
        .iter()
        .find(|case| case.id == "codex-steer-lost-result")
        .unwrap();
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let request = br#"{"jsonrpc":"2.0","id":4,"method":"turn/steer","params":{}}
"#;
    let mut input = std::io::Cursor::new(request);
    let mut output = Vec::new();
    let mut error = Vec::new();
    let mut state = super::super::WireState::new();
    super::super::execute_with_io(&steps[9], &mut state, &mut input, &mut output, &mut error)
        .unwrap();
    assert_eq!(input.position(), request.len() as u64);
    assert!(output.is_empty());
    assert_eq!(state.last_request_id, Some(4));
    assert_eq!(steps[10], json!({"op":"exit","code":17}));
}
