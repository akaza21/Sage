use super::credentials::sensitive_header;
use super::paths::{PathContext, PathMode, absolute_path};
use super::secrets::{REDACTED, SecretContext, append};
use crate::acp::contract::ContractError;

pub(crate) fn redact(
    field: &'static str,
    input: &str,
    maximum: usize,
    mode: PathMode,
) -> Result<String, ContractError> {
    let pem = super::pem::redact_pem(input);
    let input = pem.as_ref();
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
        if path && mode == PathMode::Permission {
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
            if path || sensitive_header(body, PathMode::CommandOutput) {
                let body = if path {
                    body.trim_end_matches(['\'', '"'])
                } else {
                    body
                };
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
        let body = if literal_value && self.quote.is_none() && !body.starts_with(['\'', '"']) {
            body.trim_end_matches(['\'', '"'])
        } else {
            body
        };
        let mut previous = None;
        let mut scanned_quote = self.quote.is_some();
        for (index, character) in body.char_indices() {
            if self.escaped {
                if path
                    && index + character.len_utf8() == body.len()
                    && Some(character) == self.quote
                    && absolute_path(body, PathMode::CommandOutput)
                        == Some(super::paths::PathKind::Windows)
                {
                    self.quote = None;
                }
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
