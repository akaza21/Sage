use super::secrets::{REDACTED, SecretContext, append, is_authorization_header};
use crate::acp::contract::ContractError;

pub(crate) fn redact(
    field: &'static str,
    input: &str,
    maximum: usize,
    reject_paths: bool,
) -> Result<String, ContractError> {
    let mut output = String::new();
    let mut secrets = SecretContext::default();
    let mut value = ShellValueContext::default();
    for token in input.split_inclusive(char::is_whitespace) {
        let body = token.trim_end_matches(char::is_whitespace);
        let whitespace = &token[body.len()..];
        let path = contains_absolute_path(body, reject_paths);
        let continued = value.in_value();
        let literal_value = continued || secrets.next_value();
        if path && reject_paths {
            return Err(ContractError::ForbiddenContent {
                field: "permissionCommand",
            });
        }
        let redacted = secrets.redact(body, whitespace, continued) || path;
        if !redacted && secrets.next_value() && body.starts_with('`') {
            value.markdown_wrapper = true;
        }
        if redacted || (secrets.next_value() && body.starts_with(['\'', '"'])) {
            value.track_value(body, path, literal_value);
        }
        if value.escaped && !whitespace.is_empty() {
            value.escaped = false;
            value.continued_value = true;
        }
        let replacement = if redacted || value.in_value() {
            REDACTED
        } else {
            body
        };
        if !continued {
            append(&mut output, replacement, maximum, field)?;
        }
        if !value.in_value() {
            append(&mut output, whitespace, maximum, field)?;
        }
    }
    Ok(output)
}

#[derive(Default)]
struct ShellValueContext {
    markdown_wrapper: bool,
    quote: Option<char>,
    escaped: bool,
    continued_value: bool,
    substitution_depth: usize,
}

impl ShellValueContext {
    fn in_value(&self) -> bool {
        self.quote.is_some() || self.continued_value || self.substitution_depth > 0
    }

    fn track_value(&mut self, body: &str, path: bool, literal_value: bool) {
        if body.is_empty() {
            return;
        }
        self.continued_value = false;
        let markdown_wrapper = std::mem::take(&mut self.markdown_wrapper);
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
        let body = if markdown_wrapper && self.quote.is_none() && !body.starts_with('`') {
            body.strip_suffix('`')
                .filter(|body| !body.contains('`'))
                .unwrap_or(body)
        } else {
            body
        };
        let mut previous = None;
        for (index, character) in body.char_indices() {
            if self.escaped {
                self.escaped = false;
            } else if let Some(open) = self.quote {
                if character == open {
                    self.quote = None;
                } else if open != '\'' && (character == '\\' || (character == '`' && open != '`')) {
                    self.escaped = true;
                }
            } else if character == '\\'
                || (character == '`' && index > 0 && index + 1 == body.len())
            {
                self.escaped = true;
            } else if matches!(character, '\'' | '"' | '`') {
                self.quote = Some(character);
            } else if character == '(' && (previous == Some('$') || self.substitution_depth > 0) {
                self.substitution_depth += 1;
            } else if character == ')' && self.substitution_depth > 0 {
                self.substitution_depth -= 1;
            }
            previous = Some(character);
        }
    }
}

pub(super) fn contains_absolute_path(value: &str, strict: bool) -> bool {
    if value.to_ascii_lowercase().contains("file://") {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.iter().enumerate().any(|(index, _)| {
        let previous = (index > 0).then(|| bytes[index - 1]);
        let boundary = is_path_boundary(bytes, index);
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

pub(crate) fn is_path_boundary(value: &[u8], index: usize) -> bool {
    let previous = index.checked_sub(1).map(|index| value[index]);
    let prefix = &value[..index];
    let prefix = match prefix.first() {
        Some(b'"' | b'\'' | b'`') => &prefix[1..],
        _ => prefix,
    };
    previous.is_none_or(|byte| {
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
                | b'@'
        )
    }) || (prefix.len() == 2 && prefix[0] == b'-' && prefix[1].is_ascii_alphabetic())
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
