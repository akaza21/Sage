use super::{
    Mode,
    credentials::{decode_key, sensitive_key},
    paths::absolute_path,
    secrets::append,
};
use crate::acp::contract::ContractError;
use std::borrow::Cow;

pub(super) fn redact<'a>(
    input: &'a str,
    maximum: usize,
    mode: Mode,
    field: &'static str,
) -> Result<Cow<'a, str>, ContractError> {
    let bytes = input.as_bytes();
    let mut cursor = 0;
    let mut copied = 0;
    let mut output = String::new();
    while cursor < bytes.len() {
        if bytes[cursor] != b'"' {
            cursor += 1;
            continue;
        }
        let escaped = cursor > 0 && bytes[cursor - 1] == b'\\';
        let Some(end) = quote_end(input, cursor + 1, escaped) else {
            break;
        };
        let key = decode_key(&input[cursor + 1..end - 1 - usize::from(escaped)], escaped);
        cursor = end;
        let colon =
            end + input[end..].len() - input[end..].trim_start_matches(char::is_whitespace).len();
        if bytes.get(colon) != Some(&b':') || sensitive_key(&key) {
            continue;
        }
        let after = colon + 1;
        let start = after + input[after..].len()
            - input[after..].trim_start_matches(char::is_whitespace).len();
        let escaped = input[start..].starts_with("\\\"");
        let delimiter = 1 + usize::from(escaped);
        if !escaped && bytes.get(start) != Some(&b'"') {
            continue;
        }
        let Some(end) = quote_end(input, start + delimiter, escaped) else {
            let Some(end) = super::spans::quote_end(
                input,
                start + delimiter,
                b'"',
                escaped,
                input.len(),
                Mode::Diagnostic,
            ) else {
                break;
            };
            cursor = end;
            continue;
        };
        let decoded = decode_key(&input[start + delimiter..end - delimiter], escaped);
        if mode == Mode::Permission
            && decoded
                .split_whitespace()
                .any(|token| absolute_path(token, mode).is_some())
        {
            return Err(ContractError::ForbiddenContent {
                field: "permissionCommand",
            });
        }
        let sanitized = match mode {
            Mode::Prose => super::prose::redact_prose(field, &decoded, maximum)?,
            _ => super::command::redact(field, &decoded, maximum, mode)?,
        };
        if sanitized != decoded {
            let mut encoded = sanitized.replace('\\', "\\\\").replace('"', "\\\"");
            if escaped {
                encoded = encoded.replace('\\', "\\\\");
            }
            append(
                &mut output,
                &input[copied..start + delimiter],
                maximum,
                field,
            )?;
            append(&mut output, &encoded, maximum, field)?;
            append(&mut output, &input[end - delimiter..end], maximum, field)?;
            copied = end;
        }
        cursor = end;
    }
    if copied == 0 {
        return Ok(Cow::Borrowed(input));
    }
    append(&mut output, &input[copied..], maximum, field)?;
    Ok(Cow::Owned(output))
}

fn quote_end(value: &str, cursor: usize, escaped: bool) -> Option<usize> {
    super::spans::quote_end(
        value,
        cursor,
        b'"',
        escaped,
        cursor.saturating_add(super::spans::MAX_VALUE_BYTES),
        Mode::CommandOutput,
    )
}
