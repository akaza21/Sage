use super::credentials::{
    attached_short_value, is_sensitive_prose_flag, sensitive_assignment, sensitive_key,
};
use super::paths::PathMode;
use super::secrets::{REDACTED, append};
use crate::acp::contract::ContractError;
use std::borrow::Cow;

use super::spans::{MAX_VALUE_BYTES as MAX_QUOTED_BYTES, MAX_VALUE_NEWLINES as MAX_QUOTED_LINES};
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
        let (mut end, exhausted, closed) = value_end(input, value, mode);
        if super::credentials::auth_scheme(&input[value..end]) {
            let following = end + input[end..].len()
                - input[end..].trim_start_matches(char::is_whitespace).len();
            end = value_end(input, following, mode).0;
        }
        if quote_at(bytes, value).is_none()
            && quoted_fragment(bytes, value)
            && !closed
            && !exhausted
        {
            index = end.max(index + 1);
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
    if key_start > 0 && bytes[key_start - 1] == b'/' {
        return None;
    }
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
        || matches!(bytes.get(literal_end), Some(b',' | b';'))
        || matches!(bytes.get(value), Some(b'{' | b'['));
    if key_start > 0
        && key_end == separator
        && matches!(bytes[key_start - 1], b'\'' | b'"')
        && bytes.get(value) == Some(&bytes[key_start - 1])
        && bytes.get(value + 1).is_none_or(u8::is_ascii_whitespace)
    {
        return None;
    }
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

pub(crate) fn attached_flag_end(input: &str, mode: PathMode) -> Option<usize> {
    let (start, value) = quoted_flag(input, 0)?;
    (start == 0).then(|| value_end(input, value, mode).0)
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
    let attached = (end == start + 2
        && matches!(&bytes[start..end], b"-p" | b"-P" | b"-u" | b"-U"))
        || attached_short_value(&input[start..end]);
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

fn value_end(input: &str, start: usize, mode: PathMode) -> (usize, bool, bool) {
    let bytes = input.as_bytes();
    if bytes
        .get(start)
        .is_some_and(|byte| matches!(byte, b'{' | b'['))
        && !input[start..].starts_with(REDACTED)
    {
        return match super::spans::container_end(input, start, mode) {
            Some(end) => (end, false, true),
            None => (input.len(), true, false),
        };
    }
    if quote_at(bytes, start).is_none() && !quoted_fragment(bytes, start) {
        return (super::scalars::value_end(input, start), false, false);
    }
    let mut index = start;
    let mut newlines = 0;
    let quoted = quote_at(bytes, start).is_some() || quoted_fragment(bytes, start);
    let mut closed = false;
    while index < bytes.len() {
        if let Some((quote, escaped_quote)) = quote_at(bytes, index).filter(|_| quoted) {
            let cursor = index + 1 + usize::from(escaped_quote);
            let limit = start
                .saturating_add(super::spans::byte_limit(mode))
                .min(bytes.len());
            let Some(end) =
                super::spans::quote_end(input, cursor, quote, escaped_quote, limit, mode)
            else {
                let fragment = &input[start..];
                let exhausted = fragment.len() >= MAX_QUOTED_BYTES
                    || fragment
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .take(MAX_QUOTED_LINES)
                        .count()
                        >= MAX_QUOTED_LINES;
                return (input.len(), exhausted, false);
            };
            newlines += bytes[index..end]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count();
            if mode != PathMode::Diagnostic && newlines >= MAX_QUOTED_LINES {
                return (input.len(), true, false);
            }
            index = end;
            closed = true;
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
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'$' | b'\\' | b'[' | b']')
}

fn is_value_boundary(byte: &u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'=' | b':' | b',' | b';' | b'}' | b']')
}

fn is_wrapper(byte: u8) -> bool {
    matches!(byte, b'\'' | b'"' | b'\x60' | b'\\')
}
