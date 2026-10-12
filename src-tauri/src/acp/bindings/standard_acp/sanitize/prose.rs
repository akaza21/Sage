use super::neutralized;
use crate::acp::{
    contract::ContractError,
    limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS},
    redaction::text::{self, Mode},
};

pub(crate) fn sanitized_content_output(input: &str) -> Result<String, ContractError> {
    let maximum_input = LIMITS.wire.frame_bytes as usize;
    let maximum_output = maximum_input.saturating_mul(COMMAND_OUTPUT_REDACTION_BYTES);
    if input.contains('\0') {
        return Err(ContractError::ForbiddenContent {
            field: "toolOutput",
        });
    }
    if input.len() > maximum_input {
        return Err(ContractError::InvalidLength {
            field: "toolOutput",
            minimum: 0,
            maximum: maximum_input,
        });
    }

    let input = neutralized(input, false);
    redact_prose("toolOutput", &input, maximum_output)
}

pub(super) fn redact_prose(
    field: &'static str,
    input: &str,
    maximum: usize,
) -> Result<String, ContractError> {
    text::redact(field, input, maximum, Mode::Prose)
}
