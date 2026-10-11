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
    let mut paths = PathContext::default();
    let mode = if reject_paths {
        PathMode::Permission
    } else {
        PathMode::CommandOutput
    };
    for token in input.split_inclusive(char::is_whitespace) {
        let body = token.trim_end_matches(char::is_whitespace);
        let whitespace = &token[body.len()..];
        let path = paths.redact(body, whitespace, mode);
        let continued = value.in_value();
        let literal_value = continued || secrets.next_value();
        let quoted = secrets.quoted_value_start(body).is_some();
        if path && reject_paths {
            return Err(ContractError::ForbiddenContent {
                field: "permissionCommand",
            });
        }
        let redacted = secrets.redact(body, whitespace, continued) || path || quoted;
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
        if whitespace.contains(['\n', '\r']) {
            if value.in_value() {
                secrets.finish_value();
            }
            value = ShellValueContext::default();
        }
        if !value.in_value() {
            append(&mut output, whitespace, maximum, field)?;
        }
    }
    Ok(output)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathMode {
    Prose,
    CommandOutput,
    Permission,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathKind {
    Posix,
    Windows,
}

#[derive(Default)]
pub(crate) struct PathContext {
    windows: bool,
}

impl PathContext {
    pub(crate) fn redact(&mut self, body: &str, whitespace: &str, mode: PathMode) -> bool {
        let path = absolute_path(body, mode);
        let continuation = self.windows && body.contains('\\');
        if !body.is_empty() {
            self.windows = path == Some(PathKind::Windows) || continuation;
        }
        if whitespace.contains(['\n', '\r']) {
            self.windows = false;
        }
        path.is_some() || continuation
    }
}

pub(crate) fn absolute_path(value: &str, mode: PathMode) -> Option<PathKind> {
    let bytes = value.as_bytes();
    if bytes.windows(7).enumerate().any(|(index, prefix)| {
        prefix.eq_ignore_ascii_case(b"file://")
            && (mode != PathMode::Prose
                || bytes[index + 7..].starts_with(b"/")
                || bytes[index + 7..].starts_with(b"\\\\"))
    }) {
        return Some(PathKind::Posix);
    }
    let last_non_slash = bytes.iter().rposition(|byte| *byte != b'/');
    let last_slash = bytes.iter().rposition(|byte| *byte == b'/');
    for index in 0..bytes.len() {
        if !is_path_boundary(bytes, index) {
            continue;
        }
        let rest = &bytes[index..];
        if rest.starts_with(b"\\\\")
            || (rest.len() >= 3
                && rest[0].is_ascii_alphabetic()
                && rest[1] == b':'
                && matches!(rest[2], b'\\' | b'/'))
        {
            return Some(PathKind::Windows);
        }
        if rest[0] != b'/' {
            continue;
        }
        let previous = index.checked_sub(1).map(|index| bytes[index]);
        let posix = match mode {
            PathMode::Permission => true,
            PathMode::CommandOutput => {
                rest != b"/*"
                    && (rest.len() == 1 || last_non_slash.is_some_and(|last| last >= index))
                    && (previous != Some(b'<') || !is_quoted_closing_tag(bytes, index))
            }
            PathMode::Prose => {
                rest.get(1).is_some_and(|byte| {
                    !byte.is_ascii()
                        || byte.is_ascii_alphanumeric()
                        || matches!(byte, b'.' | b'_' | b'-' | b'~')
                }) && (previous != Some(b'<') || last_slash.is_some_and(|last| last > index))
            }
        };
        if posix && !(previous == Some(b':') && rest.starts_with(b"//")) {
            return Some(PathKind::Posix);
        }
    }
    None
}

fn is_path_boundary(value: &[u8], index: usize) -> bool {
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
        || matches!(prefix, b"-isystem" | b"-iquote")
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
