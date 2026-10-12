use super::redaction::PathMode;
use super::secrets::{REDACTED, append};
use super::{is_sensitive_prose_flag, sensitive_assignment, sensitive_key};
use crate::acp::contract::ContractError;
use std::borrow::Cow;

const MAX_QUOTED_BYTES: usize = 8192;
const MAX_QUOTED_LINES: usize = 64;
pub(crate) const SECRET_LIMIT: &str = "[secret limit]";

#[derive(Default)]
pub(crate) struct ValueBudget {
    bytes: usize,
    newlines: usize,
}

impl ValueBudget {
    pub(crate) fn exhausted(&mut self, body: &str, whitespace: &str) -> bool {
        self.bytes = self
            .bytes
            .saturating_add(body.len())
            .saturating_add(whitespace.len());
        self.newlines = self
            .newlines
            .saturating_add(whitespace.bytes().filter(|byte| *byte == b'\n').count());
        self.bytes > MAX_QUOTED_BYTES || self.newlines >= MAX_QUOTED_LINES
    }
}

pub(crate) struct RedactedValues<'a> {
    pub(crate) text: Cow<'a, str>,
    pub(crate) exhausted: bool,
}

pub(crate) fn redact_values<'a>(
    field: &'static str,
    input: &'a str,
    maximum: usize,
    mode: PathMode,
) -> Result<RedactedValues<'a>, ContractError> {
    let bytes = input.as_bytes();
    let mut output = String::new();
    let mut copied = 0;
    let mut index = 0;
    while index < bytes.len() {
        let assignment = matches!(bytes[index], b'=' | b':')
            .then(|| assignment_start(input, index, mode != PathMode::Permission))
            .flatten();
        let flag = (bytes[index] == b'-' && (index == 0 || bytes[index - 1].is_ascii_whitespace()))
            .then(|| quoted_flag(input, index))
            .flatten();
        let Some((start, value)) = assignment.or(flag) else {
            index += 1;
            continue;
        };
        if start < copied || value >= bytes.len() || bytes[value..].starts_with(b"$(") {
            index += 1;
            continue;
        }
        let (end, exhausted, closed) = value_end(input, value);
        if quote_at(bytes, value).is_none()
            && quoted_fragment(bytes, value)
            && !closed
            && !exhausted
        {
            index += 1;
            continue;
        }
        append(&mut output, &input[copied..start], maximum, field)?;
        let wrapped = start == value;
        if wrapped {
            append(&mut output, "\"", maximum, field)?;
        }
        append(&mut output, REDACTED, maximum, field)?;
        if wrapped {
            append(&mut output, "\"", maximum, field)?;
        }
        if wrapped
            && mode == PathMode::CommandOutput
            && matches!(bytes.get(end), Some(b',' | b';' | b'}' | b']'))
        {
            append(&mut output, " ", maximum, field)?;
        }
        if exhausted {
            return Ok(RedactedValues {
                text: Cow::Owned(output),
                exhausted: true,
            });
        }
        copied = end;
        index = end.max(index + 1);
    }
    if copied == 0 && input.len() <= maximum {
        return Ok(RedactedValues {
            text: Cow::Borrowed(input),
            exhausted: false,
        });
    }
    append(&mut output, &input[copied..], maximum, field)?;
    Ok(RedactedValues {
        text: Cow::Owned(output),
        exhausted: false,
    })
}

fn assignment_start(
    input: &str,
    separator: usize,
    preserve_spaced_key: bool,
) -> Option<(usize, usize)> {
    let bytes = input.as_bytes();
    let mut key_end = separator;
    while key_end > 0
        && (bytes[key_end - 1].is_ascii_whitespace() || is_wrapper(bytes[key_end - 1]))
    {
        key_end -= 1;
    }
    let mut key_start = key_end;
    while key_start > 0 && is_key_byte(bytes[key_start - 1]) {
        key_start -= 1;
    }
    if key_start == key_end {
        return None;
    }
    let mut value = separator + 1;
    while value < bytes.len() && bytes[value].is_ascii_whitespace() {
        value += 1;
    }
    let key = &input[key_start..key_end];
    let literal_end = bytes[value..]
        .iter()
        .position(is_value_boundary)
        .unwrap_or(bytes.len() - value)
        + value;
    let structured = bytes[separator] == b':'
        && bytes[key_end..separator]
            .iter()
            .any(|byte| matches!(byte, b'"' | b'\''));
    if !(structured && sensitive_key(key))
        && !sensitive_assignment(
            key,
            char::from(bytes[separator]),
            &input[value..literal_end],
        )
    {
        return None;
    }
    let compound = structured
        || (key_start > 0 && matches!(bytes[key_start - 1], b'=' | b',' | b';' | b'{' | b':'))
        || matches!(bytes.get(literal_end), Some(b',' | b';'));
    if quote_at(bytes, value).is_none() && !quoted_fragment(bytes, value) && !compound {
        return None;
    }
    while key_start > 0 && is_wrapper(bytes[key_start - 1]) {
        key_start -= 1;
    }
    let spaced = bytes[separator + 1..value]
        .iter()
        .any(u8::is_ascii_whitespace)
        || bytes[key_end..separator]
            .iter()
            .any(u8::is_ascii_whitespace);
    Some((
        if spaced && preserve_spaced_key {
            value
        } else {
            key_start
        },
        value,
    ))
}

fn quoted_flag(input: &str, start: usize) -> Option<(usize, usize)> {
    let bytes = input.as_bytes();
    let mut end = start;
    while end < bytes.len()
        && !bytes[end].is_ascii_whitespace()
        && !matches!(bytes[end], b'=' | b'\'' | b'"')
    {
        end += 1;
    }
    let attached = end == start + 2 && matches!(&bytes[start..end], b"-p" | b"-P" | b"-u" | b"-U");
    if !attached && !is_sensitive_prose_flag(&input[start..end]) {
        return None;
    }
    let mut value = end;
    while value < bytes.len() && bytes[value].is_ascii_whitespace() {
        value += 1;
    }
    quote_at(bytes, value)?;
    Some((
        if attached && end == value {
            start
        } else {
            value
        },
        value,
    ))
}

fn value_end(input: &str, start: usize) -> (usize, bool, bool) {
    let bytes = input.as_bytes();
    let mut index = start;
    let mut lines = 1;
    let quoted = quote_at(bytes, start).is_some() || quoted_fragment(bytes, start);
    let mut closed = false;
    while index < bytes.len() {
        if let Some((quote, escaped_quote)) = quote_at(bytes, index).filter(|_| quoted) {
            index += 1 + usize::from(escaped_quote);
            let mut escaped = false;
            while index < bytes.len() {
                if index - start >= MAX_QUOTED_BYTES || lines > MAX_QUOTED_LINES {
                    return (input.len(), true, false);
                }
                if bytes[index] == b'\n' {
                    lines += 1;
                }
                if escaped_quote && bytes[index] == b'\\' {
                    let run_start = index;
                    while bytes.get(index) == Some(&b'\\') {
                        index += 1;
                        if index - start >= MAX_QUOTED_BYTES {
                            return (input.len(), true, false);
                        }
                    }
                    if bytes.get(index) == Some(&quote) {
                        index += 1;
                        if (index - run_start - 1) % 4 == 1 {
                            closed = true;
                            break;
                        }
                    }
                    continue;
                }
                let character = bytes[index];
                index += 1;
                if escaped {
                    escaped = false;
                } else if !escaped_quote && character == quote {
                    closed = true;
                    break;
                } else if character == b'\\' && quote != b'\'' {
                    escaped = true;
                }
            }
        } else if bytes[index].is_ascii_whitespace()
            || matches!(bytes[index], b',' | b';' | b'}' | b']')
        {
            break;
        } else {
            index += 1;
        }
    }
    (index, false, closed)
}

fn quoted_fragment(bytes: &[u8], start: usize) -> bool {
    bytes[start..]
        .iter()
        .take_while(|byte| !is_value_boundary(byte))
        .any(|byte| matches!(byte, b'\'' | b'"'))
}

fn quote_at(bytes: &[u8], index: usize) -> Option<(u8, bool)> {
    match bytes.get(index) {
        Some(quote @ (b'\'' | b'"')) => Some((*quote, false)),
        Some(b'\\') if bytes.get(index + 1) == Some(&b'"') => Some((b'"', true)),
        _ => None,
    }
}

fn is_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'$')
}

fn is_value_boundary(byte: &u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'=' | b':' | b',' | b';' | b'}' | b']')
}

fn is_wrapper(byte: u8) -> bool {
    matches!(byte, b'\'' | b'"' | b'`' | b'\\')
}
