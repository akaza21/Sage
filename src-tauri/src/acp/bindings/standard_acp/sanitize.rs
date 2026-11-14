#[path = "sanitize/prose.rs"]
mod prose;
pub(crate) use prose::sanitized_content_output;

#[cfg(test)]
#[path = "sanitize/hardening_tests.rs"]
mod hardening_tests;

#[cfg(test)]
#[path = "sanitize/regression_tests.rs"]
mod regression_tests;

#[cfg(test)]
#[path = "sanitize/review_tests.rs"]
mod review_tests;

use crate::acp::{
    contract::{ContractError, SafeText},
    redaction::text::{Mode, neutralized, redact},
};
use sage_core::limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS};

pub(crate) fn normalized_text<const MAX: usize>(
    field: &'static str,
    input: &str,
) -> Result<SafeText<MAX>, ContractError> {
    redacted_text(field, input, false)
}

pub(crate) fn normalized_label_text<const MAX: usize>(
    field: &'static str,
    input: &str,
) -> Result<SafeText<MAX>, ContractError> {
    redacted_text(field, input, true)
}

fn redacted_text<const MAX: usize>(
    field: &'static str,
    input: &str,
    strip_format: bool,
) -> Result<SafeText<MAX>, ContractError> {
    if input.len() > MAX {
        return SafeText::new(field, input);
    }
    let input = neutralized(input, strip_format);
    let output = prose::redact_prose(field, &input, MAX)?;
    SafeText::new(field, output)
}

/// Sanitizes one already frame-bounded command-output value before its display
/// suffix is retained. This deliberately has a wider intermediate bound than
/// `SafeText<65536>`: a valid oversized command output must be truncated, not
/// rejected before redaction.
pub(crate) fn sanitized_command_output(input: &str) -> Result<String, ContractError> {
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
    redact("toolOutput", &input, maximum_output, Mode::CommandOutput)
}

#[cfg(test)]
#[path = "sanitize/tests.rs"]
mod tests;
