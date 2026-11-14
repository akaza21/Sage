use super::*;

pub(crate) fn replay_native(case_id: &str, requests: &str) -> Vec<Value> {
    replay_native_before(case_id, requests, "")
}

pub(in crate::acp::fixture) fn replay_native_before(
    case_id: &str,
    requests: &str,
    before_kind: &str,
) -> Vec<Value> {
    let case = crate::acp_protocol::native_cases::JOURNEY_CASES
        .iter()
        .find(|case| case.id == case_id)
        .unwrap();
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let mut input = io::Cursor::new(requests.as_bytes());
    let mut output = Vec::new();
    let mut error = Vec::new();
    let mut state = WireState::new();
    for step in &steps {
        if !before_kind.is_empty() && step["kind"] == before_kind {
            break;
        }
        // A pause only paces a live replay; the frames are the same without it.
        if step["op"] == "pauseMs" {
            continue;
        }
        execute_with_io(step, &mut state, &mut input, &mut output, &mut error).unwrap();
    }
    assert_eq!(input.position(), requests.len() as u64);
    assert!(error.is_empty());
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[cfg(test)]
pub(in crate::acp::fixture) fn capture_steps(
    steps: &[Value],
) -> Result<(Vec<u8>, Vec<u8>), FixtureError> {
    let awaiting = steps
        .iter()
        .filter(|step| {
            matches!(
                step.get("op").and_then(Value::as_str),
                Some("awaitClient" | "readPromptBytes" | "readRequestBytes")
            )
        })
        .count();
    let mut input = io::Cursor::new(vec![b'\n'; awaiting]);
    let mut output = Vec::new();
    let mut error = Vec::new();
    let mut state = WireState::new();
    for step in steps {
        if matches!(
            crate::acp::fixture::interpreter::classify(field_str(step, "op")?)?,
            crate::acp::fixture::interpreter::StepOwner::Wire
        ) {
            execute_with_io(step, &mut state, &mut input, &mut output, &mut error)?;
        }
    }
    Ok((output, error))
}
