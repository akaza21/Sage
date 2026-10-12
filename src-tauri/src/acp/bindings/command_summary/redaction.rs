use super::secrets::{REDACTED, SecretContext, append, is_authorization_header};
use crate::acp::contract::ContractError;

pub(crate) fn redact(
    field: &'static str,
    input: &str,
    maximum: usize,
    reject_paths: bool,
) -> Result<String, ContractError> {
    let mode = if reject_paths {
        PathMode::Permission
    } else {
        PathMode::CommandOutput
    };
    let scanned = super::values::redact_values(field, input, maximum, mode)?;
    let input = &scanned.text;
    let mut output = String::new();
    let mut secrets = SecretContext::default();
    let mut value = ShellValueContext::default();
    let mut budget = super::values::ValueBudget::default();
    let mut paths = PathContext::default();
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
        let scanned_quote = if redacted || (secrets.next_value() && body.starts_with(['\'', '"'])) {
            value.track_value(body, path, literal_value)
        } else {
            false
        };
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
        if (continued || value.in_value() || quoted || scanned_quote)
            && budget.exhausted(body, whitespace)
        {
            append(&mut output, super::values::SECRET_LIMIT, maximum, field)?;
            return Ok(output);
        }
        if !value.in_value() {
            budget = super::values::ValueBudget::default();
            append(&mut output, whitespace, maximum, field)?;
        }
    }
    if scanned.exhausted {
        append(&mut output, super::values::SECRET_LIMIT, maximum, field)?;
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
    home_name: bool,
}

impl PathContext {
    pub(crate) fn redact(&mut self, body: &str, whitespace: &str, mode: PathMode) -> bool {
        let path = absolute_path(body, mode);
        let continuation = self.home_name || (self.windows && body.contains('\\'));
        if !body.is_empty() {
            if path == Some(PathKind::Windows) {
                let lower = body.to_ascii_lowercase();
                self.home_name = ["\\users\\", "\\home\\"].iter().any(|prefix| {
                    lower
                        .find(prefix)
                        .is_some_and(|index| !lower[index + prefix.len()..].contains('\\'))
                });
            } else if self.home_name && body.contains('\\') {
                self.home_name = false;
            }
            self.windows = path == Some(PathKind::Windows) || continuation;
        }
        if whitespace.contains(['\n', '\r']) {
            self.windows = false;
            self.home_name = false;
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
    let flag_start = usize::from(matches!(bytes.first(), Some(b'"' | b'\'' | b'`')));
    let glued_path = if bytes.get(flag_start) == Some(&b'-') {
        let letters = bytes[flag_start + 1..]
            .iter()
            .take_while(|byte| byte.is_ascii_alphabetic())
            .count();
        (letters > 0).then_some(flag_start + 1 + letters)
    } else {
        None
    };
    let short_path = bytes.get(flag_start).is_some_and(|byte| *byte == b'-')
        && bytes
            .get(flag_start + 1)
            .is_some_and(u8::is_ascii_alphabetic);
    for index in 0..bytes.len() {
        if glued_path != Some(index)
            && !(short_path && index == flag_start + 2)
            && !is_path_boundary(bytes, index)
        {
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
                    && (plain_slash_path(rest)
                        || (matches!(rest.get(1), Some(b'/' | b'*'))
                            && last_slash.is_some_and(|last| last > index + 1)))
                    && last_non_slash.is_some_and(|last| last >= index)
                    && (previous != Some(b'<') || !is_closing_tag(bytes, index))
            }
            PathMode::Prose => {
                plain_slash_path(rest)
                    && (previous != Some(b'<') || last_slash.is_some_and(|last| last > index))
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
    })
}

fn plain_slash_path(rest: &[u8]) -> bool {
    rest.get(1).is_some_and(|byte| {
        !byte.is_ascii()
            || byte.is_ascii_alphanumeric()
            || matches!(byte, b'.' | b'_' | b'-' | b'~')
    })
}

fn is_closing_tag(value: &[u8], slash: usize) -> bool {
    let name_end = value[slash + 1..]
        .iter()
        .position(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'-' | b':'));
    name_end.is_some_and(|end| {
        end > 0
            && value.get(slash + end + 1) == Some(&b'>')
            && (slash + end + 2 == value.len()
                || matches!(value.get(slash + end + 2), Some(b'\'' | b'"')))
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

    fn track_value(&mut self, body: &str, path: bool, literal_value: bool) -> bool {
        if body.is_empty() {
            return false;
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
        let mut scanned_quote = self.quote.is_some();
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
                scanned_quote = true;
                self.quote = Some(character);
            } else if character == '(' && (previous == Some('$') || self.substitution_depth > 0) {
                self.substitution_depth += 1;
            } else if character == ')' && self.substitution_depth > 0 {
                self.substitution_depth -= 1;
            }
            previous = Some(character);
        }
        scanned_quote
    }
}
