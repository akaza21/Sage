//! How Codex text is redacted: replies and notices as every harness's are,
//! tool content with its paths kept private, and command output token by
//! token.

use sage_core::limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS};

use super::IngressError;
use crate::acp::bindings::standard_acp::sanitize::sanitized_content_output;
use crate::acp::redaction::text::reply_text;

/// Harness prose, redacted as every harness's replies and notices are.
pub(in crate::acp::bindings::codex) fn sanitize(value: &str) -> String {
    reply_text(value)
}

/// Tool content, which keeps absolute paths private as every harness's does.
pub(in crate::acp::bindings::codex) fn sanitize_tool_text(value: &str) -> String {
    sanitized_content_output(value).unwrap_or_else(|_| "[redacted]".to_owned())
}

/// Like [`sanitize`], but a token naming an absolute path becomes `[path]`,
/// for summaries of commands that keep their paths private.
pub(in crate::acp::bindings::codex) fn sanitize_hiding_paths(value: &str) -> String {
    sanitize_with_limit(value, usize::MAX, false).unwrap_or_default()
}

pub(in crate::acp::bindings::codex) fn sanitized_command_output(
    value: &str,
) -> Result<String, IngressError> {
    let maximum_input = LIMITS.wire.frame_bytes as usize;
    if value.len() > maximum_input {
        return Err(IngressError::Content);
    }
    sanitize_with_limit(
        value,
        maximum_input.saturating_mul(COMMAND_OUTPUT_REDACTION_BYTES),
        true,
    )
    .ok_or(IngressError::Content)
}

pub(in crate::acp::bindings::codex) fn sanitized_command_line(
    value: &str,
) -> Result<String, IngressError> {
    let flattened: String = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let sanitized = sanitized_command_output(&flattened)?;
    let mut line = String::new();
    for token in sanitized.split_whitespace() {
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(token);
    }
    Ok(line)
}

fn sanitize_with_limit(value: &str, maximum: usize, command_output: bool) -> Option<String> {
    let mut output = if command_output {
        String::new()
    } else {
        String::with_capacity(value.len().min(262_144))
    };
    let mut redact_next = false;
    for part in value.split_inclusive(char::is_whitespace) {
        let token = part.trim_end_matches(char::is_whitespace);
        let whitespace = &part[token.len()..];
        if command_output && token.is_empty() {
            append_command_output(&mut output, whitespace, maximum)?;
            continue;
        }
        if command_output {
            let (replacement, command_output_next) = command_output_token(token, redact_next);
            append_command_output(&mut output, replacement, maximum)?;
            append_command_output(&mut output, whitespace, maximum)?;
            redact_next = command_output_next;
        } else {
            // Keep the generic sanitizer's historical `Bearer` transition:
            // `Bearer:` redacts itself but never carries into a later token.
            let bare = token.trim_matches(command_output_trim_character);
            let embedded_bearer = bare
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer:"));
            let replacement = if redact_next || embedded_bearer || bare.contains("sk-") {
                "[redacted]"
            } else if contains_absolute_path(token, true) {
                "[path]"
            } else {
                token
            };
            output.push_str(replacement);
            output.push_str(whitespace);
            redact_next = bare.eq_ignore_ascii_case("bearer");
        }
    }
    Some(output)
}

/// Callers that receive output in frames must keep incomplete tokens private
/// until a whitespace boundary, then pass the complete token and the carried
/// Bearer state here.
pub(in crate::acp::bindings::codex) fn command_output_token(
    token: &str,
    redact_next: bool,
) -> (&str, bool) {
    let bare = token.trim_matches(command_output_trim_character);
    let embedded_bearer = bare
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer:"));
    let replacement = if redact_next || embedded_bearer || bare.contains("sk-") {
        "[redacted]"
    } else if contains_absolute_path(token, false) {
        "[path]"
    } else {
        token
    };
    (
        replacement,
        bare.eq_ignore_ascii_case("bearer") || bare.eq_ignore_ascii_case("bearer:"),
    )
}

fn command_output_trim_character(character: char) -> bool {
    matches!(
        character,
        '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';'
    )
}

fn append_command_output(output: &mut String, value: &str, maximum: usize) -> Option<()> {
    let next = output.len().checked_add(value.len())?;
    if next > maximum {
        return None;
    }
    output.try_reserve(value.len()).ok()?;
    output.push_str(value);
    Some(())
}

/// In prose a session-action command such as `/review` reads as itself;
/// command output keeps every rooted token private.
fn contains_absolute_path(token: &str, prose: bool) -> bool {
    let lower = token.to_ascii_lowercase();
    if lower.contains("file:///") || lower.contains("file://\\\\") {
        return true;
    }
    let bytes = token.as_bytes();
    for index in 0..bytes.len() {
        let boundary = index == 0
            || matches!(
                bytes[index - 1],
                b'"' | b'\'' | b'`' | b'(' | b'[' | b'{' | b'<' | b'=' | b',' | b';'
            );
        if !boundary {
            continue;
        }
        let rest = &bytes[index..];
        if rest.starts_with(b"\\\\")
            || (rest.starts_with(b"/")
                && !rest.starts_with(b"//")
                && !(prose
                    && crate::acp::redaction::text::paths::is_session_action_command(
                        rest,
                        index.checked_sub(1).map(|previous| bytes[previous]),
                    )))
            || (rest.len() >= 3
                && rest[0].is_ascii_alphabetic()
                && rest[1] == b':'
                && matches!(rest[2], b'\\' | b'/'))
        {
            return true;
        }
    }
    false
}
