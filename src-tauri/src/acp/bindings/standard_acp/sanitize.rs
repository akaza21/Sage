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

use crate::acp::redaction::text::{Mode, clip_display, neutralized, redact};
use sage_acp_protocol::contract::{ContractError, SafeText};
use sage_core::limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS};

pub(crate) fn normalized_text<const MAX: usize>(
    field: &'static str,
    input: &str,
) -> Result<SafeText<MAX>, ContractError> {
    if input.len() > MAX {
        return SafeText::new(field, input);
    }
    let input = neutralized(input, false);
    SafeText::new(field, prose::redact_prose(field, &input, MAX)?)
}

/// A label is display text, so one longer than `MAX` is clipped after its
/// whole text is redacted rather than refused.
pub(crate) fn normalized_label_text<const MAX: usize>(
    field: &'static str,
    input: &str,
) -> Result<SafeText<MAX>, ContractError> {
    let input = neutralized(input, true);
    let redacted = prose::redact_prose(
        field,
        &input,
        input.len().saturating_mul(COMMAND_OUTPUT_REDACTION_BYTES),
    )?;
    SafeText::new(field, clip_display(&redacted, MAX))
}

/// This deliberately has a wider intermediate bound than `SafeText<65536>`: a valid oversized
/// command output must be truncated, not rejected before redaction.
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
