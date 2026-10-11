use std::borrow::Cow;

use crate::acp::{
    bindings::command_summary::{is_sensitive_prose_flag, is_sensitive_prose_token},
    contract::{ContractError, SafeText},
    limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS},
};

const REDACTED: &str = "[redacted]";

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

pub(crate) fn neutralized_label_text<const MAX: usize>(
    field: &'static str,
    input: &str,
) -> Result<SafeText<MAX>, ContractError> {
    if input.len() > MAX {
        return SafeText::new(field, input);
    }
    SafeText::new(field, neutralized(input, true).as_ref())
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
    let mut output = String::with_capacity(input.len().min(MAX));
    let mut secrets = SecretContext::default();
    for token in input.split_inclusive(char::is_whitespace) {
        let (body, whitespace) = split_trailing_whitespace(token);
        if secrets.redact(body, whitespace) {
            output.push_str(REDACTED);
        } else {
            output.push_str(body);
        }
        output.push_str(whitespace);
    }
    SafeText::new(field, output)
}

fn neutralized(input: &str, strip_format: bool) -> Cow<'_, str> {
    if !input.chars().any(|character| {
        (strip_format && is_stripped_format(character)) || is_neutralized_control(character)
    }) {
        return Cow::Borrowed(input);
    }
    let mut output = String::with_capacity(input.len());
    for character in input.chars() {
        if strip_format && is_stripped_format(character) {
            continue;
        }
        output.push(if is_neutralized_control(character) {
            ' '
        } else {
            character
        });
    }
    Cow::Owned(output)
}

fn is_neutralized_control(character: char) -> bool {
    matches!(character, '\u{1}'..='\u{1f}' | '\u{7f}'..='\u{9f}')
        && !matches!(character, '\t' | '\n' | '\r')
}

fn is_stripped_format(character: char) -> bool {
    matches!(
        character,
        '\u{ad}'
            | '\u{61c}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_comment_markers_survive_while_embedded_paths_stay_redacted() {
        let source =
            sanitized_command_output("/// doc // line /* block */ render(\"</div>\"); a//b")
                .unwrap();
        assert_eq!(
            source,
            "/// doc // line /* block */ render(\"</div>\"); a//b"
        );
        let embedded =
            sanitized_command_output("open(\"/home/probe/secret.txt\") </home/probe/x>").unwrap();
        assert_eq!(embedded, "[redacted] [redacted]");
    }

    #[test]
    fn redacts_secret_and_native_path_tokens() {
        let value = normalized_text::<512>(
            "fixture",
            r"result sk-ant-secret C:\private\file.txt /private/file Bearer abc123 safe",
        )
        .unwrap();
        assert_eq!(
            value.as_str(),
            "result [redacted] [redacted] [redacted] Bearer [redacted] safe"
        );
    }

    #[test]
    fn an_embedded_absolute_path_behind_a_flag_or_key_is_redacted() {
        let value = normalized_text::<512>(
            "fixture",
            r"Write --target=C:\Users\Alice\secret.txt note:/home/probe/x tail",
        )
        .unwrap();
        assert_eq!(value.as_str(), "Write [redacted] [redacted] tail");
    }

    #[test]
    fn a_web_url_survives_the_embedded_path_redaction() {
        let value =
            normalized_text::<512>("fixture", "Fetch https://example.com/docs and http://a.b/c")
                .unwrap();
        assert_eq!(
            value.as_str(),
            "Fetch https://example.com/docs and http://a.b/c"
        );
    }

    #[test]
    fn control_and_bidi_characters_are_neutralized_before_redaction() {
        let value = normalized_label_text::<512>(
            "fixture",
            "Write\u{7}hr6.txt\u{202e}\u{200b}\u{feff} tail",
        )
        .unwrap();
        assert_eq!(value.as_str(), "Write hr6.txt tail");
    }

    #[test]
    fn tabs_and_newlines_and_nul_survive_neutralization() {
        let value = normalized_text::<512>("fixture", "a\tb\nc").unwrap();
        assert_eq!(value.as_str(), "a\tb\nc");
        assert!(normalized_text::<512>("fixture", "a\u{0}b").is_err());
    }

    #[test]
    fn joiners_survive_message_text_while_labels_still_strip_bidi_overrides() {
        let message = "\u{1f469}\u{200d}\u{1f4bb} \u{62f}\u{200c}\u{646} tail";
        let value = normalized_text::<512>("text", message).unwrap();
        assert_eq!(value.as_str(), message);
        let label = normalized_label_text::<512>("toolLabel", message).unwrap();
        assert_eq!(label.as_str(), "\u{1f469}\u{1f4bb} \u{62f}\u{646} tail");
        let overridden = normalized_text::<512>("text", "a\u{202e}b").unwrap();
        assert_eq!(overridden.as_str(), "a\u{202e}b");
    }

    #[test]
    fn claude_step_08_prose_tool_label_and_read_output_survive_shell_syntax() {
        let prose = "Here's the file's exact contents:\n```text\nread-only harness probe\n```";
        assert_eq!(
            normalized_text::<512>("text", prose).unwrap().as_str(),
            prose
        );
        let label = "Search for `alpha.txt` in the project's files";
        assert_eq!(
            normalized_label_text::<512>("toolLabel", label)
                .unwrap()
                .as_str(),
            label
        );
        let output = "```text\n1\tread-only harness probe\n3\t```";
        assert_eq!(
            normalized_text::<512>("toolOutput", output)
                .unwrap()
                .as_str(),
            output
        );
        assert_eq!(sanitized_command_output(output).unwrap(), output);
        let provider_error = "Internal error: Error from provider (Console): OpenCode's free tier can only be used from within OpenCode";
        assert_eq!(
            normalized_text::<512>("errorMessage", provider_error)
                .unwrap()
                .as_str(),
            provider_error
        );
    }

    #[test]
    fn prose_and_output_redact_only_secret_tokens_and_paths() {
        let input = "Here's token=private Bearer abc123 https://user:pass@example.com/x /home/probe/key.txt safe";
        let result = normalized_text::<512>("text", input).unwrap();
        assert_eq!(
            result.as_str(),
            "Here's [redacted] Bearer [redacted] [redacted] [redacted] safe"
        );
        assert_eq!(sanitized_command_output(input).unwrap(), result.as_str());
    }

    #[test]
    fn authorization_schemes_and_mixed_case_bearer_never_expose_values() {
        for (input, expected) in [
            ("bearer abc123 tail", "bearer [redacted] tail"),
            (
                "\"Bearer\", \t\r\nopaque-command-credential tail",
                "\"Bearer\", \t\r\n[redacted] tail",
            ),
            (
                "Authorization: Basic abc123\nordinary prose",
                "Authorization: [redacted] [redacted]\nordinary prose",
            ),
            (
                "authorization: Digest username=private nonce=secret\nnext",
                "authorization: [redacted] [redacted] [redacted]\nnext",
            ),
            (
                "Proxy-Authorization: Token abc123\nnext",
                "Proxy-Authorization: [redacted] [redacted]\nnext",
            ),
            (
                "Authorization: Bearer\nordinary prose",
                "Authorization: [redacted]\nordinary prose",
            ),
            (
                "Authorization: --token\nordinary prose",
                "Authorization: [redacted]\nordinary prose",
            ),
        ] {
            assert_eq!(
                normalized_text::<512>("text", input).unwrap().as_str(),
                expected
            );
            assert_eq!(sanitized_command_output(input).unwrap(), expected);
        }
    }
}
