use super::*;

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
