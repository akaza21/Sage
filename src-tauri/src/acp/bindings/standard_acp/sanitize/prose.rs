use super::neutralized;
use crate::acp::{
    bindings::command_summary::{
        redaction::is_path_boundary,
        secrets::{REDACTED, SecretContext, append},
    },
    contract::ContractError,
    limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS},
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
    maximum_output: usize,
) -> Result<String, ContractError> {
    let mut output = String::new();
    let mut secrets = SecretContext::default();
    for token in input.split_inclusive(char::is_whitespace) {
        let (body, whitespace) = split_trailing_whitespace(token);
        if body.is_empty() {
            secrets.redact(body, whitespace, false);
            append(&mut output, whitespace, maximum_output, field)?;
            continue;
        }
        let replacement = if secrets.redact(body, whitespace, false)
            || is_absolute_native_path(body)
            || contains_command_absolute_path(body)
        {
            REDACTED
        } else {
            body
        };
        append(&mut output, replacement, maximum_output, field)?;
        append(&mut output, whitespace, maximum_output, field)?;
    }
    Ok(output)
}

fn contains_command_absolute_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("file:///") || lower.contains("file://\\\\") {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.iter().enumerate().any(|(index, _)| {
        let previous = (index > 0).then(|| bytes[index - 1]);
        let boundary = is_path_boundary(bytes, index);
        let rest = &bytes[index..];
        boundary
            && (rest.starts_with(b"\\\\")
                || (starts_posix_path(rest)
                    && (previous != Some(b'<') || rest[1..].contains(&b'/')))
                || (rest.len() >= 3
                    && rest[0].is_ascii_alphabetic()
                    && rest[1] == b':'
                    && matches!(rest[2], b'\\' | b'/')))
    })
}

fn starts_posix_path(value: &[u8]) -> bool {
    value.first() == Some(&b'/')
        && value.get(1).is_some_and(|byte| {
            !byte.is_ascii()
                || byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'_' | b'-' | b'~')
        })
}

fn split_trailing_whitespace(value: &str) -> (&str, &str) {
    let split = value
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index))
        .unwrap_or(value.len());
    value.split_at(split)
}

fn is_absolute_native_path(value: &str) -> bool {
    let trimmed = value.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':'
        )
    });
    starts_posix_path(trimmed.as_bytes())
        || trimmed.starts_with("\\\\")
        || (trimmed.as_bytes().get(1) == Some(&b':')
            && trimmed
                .as_bytes()
                .get(2)
                .is_some_and(|separator| matches!(separator, b'\\' | b'/'))
            && trimmed
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic))
}
