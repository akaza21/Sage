use super::WireError;
use crate::acp::{
    contract::SafeText,
    limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS},
    redaction::text::{self, Mode},
};

pub(crate) fn normalized_text<const MAX: usize>(
    field: &'static str,
    text: &str,
) -> Result<SafeText<MAX>, WireError> {
    if text.len() > MAX || text.contains('\0') {
        return Err(WireError::TextLimit);
    }
    let result = text::redact(field, text, MAX, Mode::Prose).map_err(|_| WireError::TextLimit)?;
    SafeText::new(field, result).map_err(|_| WireError::TextLimit)
}

/// Sanitizes one already frame-bounded command-output value before its display
/// suffix is retained. General `normalized_text` callers retain their strict
/// `SafeText` maximum; oversized command output is valid and is capped later.
pub(crate) fn sanitized_command_output(input: &str) -> Result<String, WireError> {
    let maximum_input = LIMITS.wire.frame_bytes as usize;
    let maximum_output = maximum_input.saturating_mul(COMMAND_OUTPUT_REDACTION_BYTES);
    if input.len() > maximum_input || input.contains('\0') {
        return Err(WireError::TextLimit);
    }

    text::redact("toolOutput", input, maximum_output, Mode::CommandOutput)
        .map_err(|_| WireError::TextLimit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_and_prose_use_shared_credential_and_path_detection() {
        for (input, secret) in [
            ("before --token abc after", "abc"),
            ("before TOKEN='private value' after", "private"),
            ("before >/home/alice/x after", "alice"),
            (r"before -isystemC:\Users\Jane\inc after", "Jane"),
            ("before C:/Users/Jane Doe/x after", "Doe"),
            (r#"before password={"a":"b","c":"d"} after"#, r#""c":"d""#),
        ] {
            for actual in [
                sanitized_command_output(input).unwrap(),
                normalized_text::<512>("text", input)
                    .unwrap()
                    .as_str()
                    .to_owned(),
            ] {
                assert!(!actual.contains(secret), "{input}: {actual}");
                assert!(
                    actual.starts_with("before ") && actual.ends_with(" after"),
                    "{input}: {actual}"
                );
            }
        }
    }

    #[test]
    fn admission_and_redaction_expansion_remain_bounded() {
        assert!(normalized_text::<8>("text", "--token a").is_err());
        assert!(normalized_text::<8>("text", "\0").is_err());
        assert!(sanitized_command_output("\0").is_err());
        assert!(
            sanitized_command_output(&"a".repeat(LIMITS.wire.frame_bytes as usize + 1)).is_err()
        );
        let input = "p=a ".repeat(20_000);
        let shown = sanitized_command_output(&input).unwrap();
        assert!(!shown.contains("p=a"));
        assert!(shown.len() <= input.len() * COMMAND_OUTPUT_REDACTION_BYTES);
    }
}
