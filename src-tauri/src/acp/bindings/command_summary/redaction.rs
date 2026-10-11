use super::{is_sensitive_prose_flag, is_sensitive_prose_token};
use crate::acp::contract::ContractError;

const REDACTED: &str = "[redacted]";

pub(crate) fn redact(
    field: &'static str,
    input: &str,
    maximum: usize,
    reject_paths: bool,
) -> Result<String, ContractError> {
    let mut output = String::new();
    let mut secrets = SecretContext::default();
    for token in input.split_inclusive(char::is_whitespace) {
        let body = token.trim_end_matches(char::is_whitespace);
        let whitespace = &token[body.len()..];
        let path = contains_absolute_path(body, reject_paths);
        let continued = secrets.in_value();
        if path && reject_paths {
            return Err(ContractError::ForbiddenContent {
                field: "permissionCommand",
            });
        }
        let replacement = if secrets.redact(body, whitespace, path) {
            REDACTED
        } else {
            body
        };
        if !continued {
            append(&mut output, replacement, maximum, field)?;
        }
        if !secrets.in_value() {
            append(&mut output, whitespace, maximum, field)?;
        }
    }
    Ok(output)
}

#[derive(Default)]
struct SecretContext {
    next_value: bool,
    authorization_line: bool,
    quote: Option<char>,
    escaped: bool,
    continued_value: bool,
    substitution_depth: usize,
}

impl SecretContext {
    fn in_value(&self) -> bool {
        self.quote.is_some() || self.continued_value || self.substitution_depth > 0
    }

    fn redact(&mut self, body: &str, whitespace: &str, path: bool) -> bool {
        if self.next_value && body == "<" {
            return false;
        }
        let was_authorization_line = self.authorization_line;
        let structural_flag = self.next_value
            && !self.in_value()
            && !body.starts_with(['\'', '"', '`'])
            && is_sensitive_prose_flag(body);
        let literal_value = self.in_value() || self.next_value;
        let redacted = !body.is_empty()
            && (self.authorization_line
                || self.next_value
                || self.in_value()
                || (is_authorization_header(body)
                    && !body.eq_ignore_ascii_case("authorization:")
                    && !body.eq_ignore_ascii_case("proxy-authorization:"))
                || is_sensitive_prose_token(body)
                || path);
        if !body.is_empty() {
            self.continued_value = false;
            if redacted {
                self.track_value(body, path, literal_value);
            }
            self.next_value = structural_flag
                || (!literal_value
                    && (is_bearer_marker(body)
                        || is_sensitive_prose_flag(body)
                        || body
                            .split_once('=')
                            .is_some_and(|(_, value)| is_bearer_marker(value))));
            if !literal_value && is_authorization_header(body) {
                self.authorization_line = true;
            }
        }
        if self.escaped && !whitespace.is_empty() {
            self.escaped = false;
            self.continued_value = true;
        }
        if whitespace.contains(['\n', '\r']) {
            self.authorization_line = false;
            if was_authorization_line || is_authorization_header(body) {
                self.next_value = false;
            }
        }
        redacted
    }

    fn track_value(&mut self, body: &str, path: bool, literal_value: bool) {
        let body = if literal_value || body.starts_with(['\'', '"']) {
            body
        } else {
            let body = body
                .strip_prefix('`')
                .and_then(|body| body.strip_suffix('`'))
                .unwrap_or(body);
            if path || is_authorization_header(body) {
                body.find(['\'', '"'])
                    .map(|index| &body[index..])
                    .unwrap_or(body)
            } else {
                body.split_once('=')
                    .or_else(|| body.split_once(':'))
                    .map(|(_, value)| value)
                    .unwrap_or(body)
            }
        };
        let mut previous = None;
        for character in body.chars() {
            if self.escaped {
                self.escaped = false;
            } else if let Some(open) = self.quote {
                if character == open {
                    self.quote = None;
                } else if open != '\'' && (character == '\\' || (character == '`' && open != '`')) {
                    self.escaped = true;
                }
            } else if matches!(character, '\'' | '"' | '`') {
                self.quote = Some(character);
            } else if matches!(character, '\\' | '`') {
                self.escaped = true;
            } else if character == '(' && (previous == Some('$') || self.substitution_depth > 0) {
                self.substitution_depth += 1;
            } else if character == ')' && self.substitution_depth > 0 {
                self.substitution_depth -= 1;
            }
            previous = Some(character);
        }
    }
}

fn is_authorization_header(body: &str) -> bool {
    let header = body.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`'));
    let header = header.to_ascii_lowercase();
    header.contains("authorization:")
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

fn append(
    output: &mut String,
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), ContractError> {
    let invalid_length = || ContractError::InvalidLength {
        field,
        minimum: 0,
        maximum,
    };
    let next = output
        .len()
        .checked_add(value.len())
        .ok_or_else(invalid_length)?;
    if next > maximum {
        return Err(invalid_length());
    }
    output
        .try_reserve(value.len())
        .map_err(|_| invalid_length())?;
    output.push_str(value);
    Ok(())
}

pub(super) fn contains_absolute_path(value: &str, strict: bool) -> bool {
    if value.to_ascii_lowercase().contains("file://") {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.iter().enumerate().any(|(index, _)| {
        let previous = (index > 0).then(|| bytes[index - 1]);
        let boundary = previous.is_none_or(|byte| {
            matches!(
                byte,
                b'"' | b'\''
                    | b'`'
                    | b'('
                    | b'['
                    | b'{'
                    | b'<'
                    | b'>'
                    | b'|'
                    | b';'
                    | b'&'
                    | b'='
                    | b','
                    | b':'
                    | b'\\'
            )
        });
        let rest = &bytes[index..];
        boundary
            && (rest.starts_with(b"\\\\")
                || (starts_posix_path(rest, strict)
                    && !(previous == Some(b':') && rest.starts_with(b"//"))
                    && (strict || previous != Some(b'<') || !is_quoted_closing_tag(bytes, index)))
                || (rest.len() >= 3
                    && rest[0].is_ascii_alphabetic()
                    && rest[1] == b':'
                    && matches!(rest[2], b'\\' | b'/')))
    })
}

fn starts_posix_path(value: &[u8], strict: bool) -> bool {
    value.first() == Some(&b'/')
        && (strict
            || (value != b"/*" && (value.len() == 1 || !value.iter().all(|byte| *byte == b'/'))))
}

fn is_quoted_closing_tag(value: &[u8], slash: usize) -> bool {
    let Some(quote) = slash.checked_sub(2).and_then(|index| value.get(index)) else {
        return false;
    };
    if !matches!(quote, b'\'' | b'"') {
        return false;
    }
    let name_end = value[slash + 1..]
        .iter()
        .position(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'-' | b':'));
    name_end.is_some_and(|end| {
        end > 0
            && value.get(slash + end + 1) == Some(&b'>')
            && value.get(slash + end + 2) == Some(quote)
    })
}
