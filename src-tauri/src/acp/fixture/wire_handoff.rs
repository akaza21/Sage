use std::io::Read;

use super::FixtureError;

pub(super) fn await_prompt(input: &mut impl Read) -> Result<(), FixtureError> {
    let mut bytes = Vec::new();
    for _ in 0..crate::acp::limits::LIMITS.wire.frame_bytes {
        let mut byte = [0];
        if input.read(&mut byte).map_err(|_| FixtureError::Io)? == 0 {
            return Err(FixtureError::InvalidArguments);
        }
        if byte[0] == b'\n' {
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| FixtureError::InvalidArguments)?;
            let text = value
                .pointer("/params/text")
                .and_then(serde_json::Value::as_str)
                .ok_or(FixtureError::InvalidArguments)?;
            if value["method"] != "fixture/prompt"
                || text.matches("Quoted untrusted historical context").count() != 1
                || !text.contains("fixture text")
                || !text.contains("\"role\":\"user\"")
                || !text.ends_with("Current user request:\nread only")
            {
                return Err(FixtureError::InvalidArguments);
            }
            return Ok(());
        }
        bytes.push(byte[0]);
    }
    Err(FixtureError::ExpandedLimit)
}
