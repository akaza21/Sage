use super::neutralized;
use crate::acp::{
    bindings::command_summary::{is_sensitive_prose_flag, is_sensitive_prose_token},
    contract::ContractError,
    limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS},
};

const REDACTED: &str = "[redacted]";

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

    let mut output = String::new();
    let mut secrets = SecretContext::default();
    let input = neutralized(input, false);
    for token in input.split_inclusive(char::is_whitespace) {
        let (body, whitespace) = split_trailing_whitespace(token);
        if body.is_empty() {
            secrets.redact(body, whitespace);
            append_command_output(&mut output, whitespace, maximum_output)?;
            continue;
        }
        let replacement = if secrets.redact(body, whitespace) {
            REDACTED
        } else {
            body
        };
        append_command_output(&mut output, replacement, maximum_output)?;
        append_command_output(&mut output, whitespace, maximum_output)?;
    }
    Ok(output)
}

#[derive(Default)]
struct SecretContext {
    next_value: bool,
    authorization_line: bool,
}

impl SecretContext {
    fn redact(&mut self, body: &str, whitespace: &str) -> bool {
        let was_authorization_line = self.authorization_line;
        let redacted = !body.is_empty()
            && (self.authorization_line
                || self.next_value
                || is_secret(body)
                || is_absolute_native_path(body)
                || contains_command_absolute_path(body));
        if !body.is_empty() {
            self.next_value = is_bearer_marker(body) || is_sensitive_prose_flag(body);
            if is_authorization_header(body) {
                self.authorization_line = true;
            }
        }
        if whitespace.contains(['\n', '\r']) {
            self.authorization_line = false;
            if was_authorization_line || is_authorization_header(body) {
                self.next_value = false;
            }
        }
        redacted
    }
}

fn is_authorization_header(body: &str) -> bool {
    let header = body.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`'));
    let header = header.to_ascii_lowercase();
    header.starts_with("authorization:") || header.starts_with("proxy-authorization:")
}

fn is_bearer_marker(value: &str) -> bool {
    let trimmed = value.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    trimmed.eq_ignore_ascii_case("bearer") || trimmed.eq_ignore_ascii_case("bearer:")
}

fn append_command_output(
    output: &mut String,
    value: &str,
    maximum: usize,
) -> Result<(), ContractError> {
    let next = output
        .len()
        .checked_add(value.len())
        .ok_or(ContractError::InvalidLength {
            field: "toolOutput",
            minimum: 0,
            maximum,
        })?;
    if next > maximum {
        return Err(ContractError::InvalidLength {
            field: "toolOutput",
            minimum: 0,
            maximum,
        });
    }
    output
        .try_reserve(value.len())
        .map_err(|_| ContractError::InvalidLength {
            field: "toolOutput",
            minimum: 0,
            maximum,
        })?;
    output.push_str(value);
    Ok(())
}

fn contains_command_absolute_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("file:///") || lower.contains("file://\\\\") {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.iter().enumerate().any(|(index, _)| {
        let previous = (index > 0).then(|| bytes[index - 1]);
        let boundary = previous.is_none_or(|byte| {
            matches!(
                byte,
                b'"' | b'\'' | b'`' | b'(' | b'[' | b'{' | b'<' | b'=' | b',' | b';' | b':'
            )
        });
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
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'~')
        })
}

fn split_trailing_whitespace(value: &str) -> (&str, &str) {
    let split = value
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index))
        .unwrap_or(value.len());
    value.split_at(split)
}

fn is_secret(value: &str) -> bool {
    is_sensitive_prose_token(value)
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
