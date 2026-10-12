use super::{
    paths::{PathContext, PathMode},
    secrets::{REDACTED, SecretContext, append},
};
use crate::acp::contract::ContractError;

pub(crate) fn redact_prose(
    field: &'static str,
    input: &str,
    maximum_output: usize,
) -> Result<String, ContractError> {
    let pem = super::pem::redact_pem(input);
    let input = pem.as_ref();
    let scanned = super::values::redact_values(field, input, maximum_output, PathMode::Prose)?;
    let input = &scanned.text;
    let mut output = String::new();
    let mut secrets = SecretContext::default();
    let mut value = QuotedValue::default();
    let mut budget = super::values::ValueBudget::default();
    let mut paths = PathContext::default();
    for token in input.split_inclusive(char::is_whitespace) {
        let (mut body, whitespace) = split_trailing_whitespace(token);
        let mut quoted_token = false;
        while !body.is_empty() {
            if let Some(suffix) = body
                .strip_prefix(REDACTED)
                .filter(|_| !secrets.protects_value() && value.quote.is_none())
            {
                secrets.finish_value();
                append(&mut output, REDACTED, maximum_output, field)?;
                body = suffix.trim_start_matches([',', ';']);
                append(
                    &mut output,
                    &suffix[..suffix.len() - body.len()],
                    maximum_output,
                    field,
                )?;
                continue;
            }
            let continued = value.quote.is_some();
            let opening = if continued {
                None
            } else {
                secrets.quoted_value_start(body)
            };
            let quoted = continued || opening.is_some();
            quoted_token |= quoted;
            let suffix = if quoted {
                value.suffix(body, opening)
            } else {
                ""
            };
            let prefix = &body[..body.len() - suffix.len()];
            let secret = secrets.redact(prefix, "", continued);
            let path = paths.redact(prefix, "", PathMode::Prose);
            if !continued {
                let replacement = if secret || path || quoted {
                    REDACTED
                } else {
                    prefix
                };
                append(&mut output, replacement, maximum_output, field)?;
            }
            if quoted && value.quote.is_none() {
                secrets.finish_value();
            }
            body = suffix.trim_start_matches([',', ';']);
            append(
                &mut output,
                &suffix[..suffix.len() - body.len()],
                maximum_output,
                field,
            )?;
        }
        secrets.redact("", whitespace, false);
        paths.redact("", whitespace, PathMode::Prose);
        if !whitespace.is_empty() {
            value.escaped = false;
        }
        if (value.quote.is_some() || quoted_token)
            && budget.exhausted(token.trim_end_matches(char::is_whitespace), whitespace)
        {
            append(
                &mut output,
                super::values::SECRET_LIMIT,
                maximum_output,
                field,
            )?;
            return Ok(output);
        }
        if value.quote.is_none() {
            budget = super::values::ValueBudget::default();
            append(&mut output, whitespace, maximum_output, field)?;
        }
    }
    if scanned.exhausted {
        append(
            &mut output,
            super::values::SECRET_LIMIT,
            maximum_output,
            field,
        )?;
    }
    Ok(output)
}

#[derive(Default)]
struct QuotedValue {
    quote: Option<char>,
    escaped: bool,
}

impl QuotedValue {
    fn suffix<'a>(&mut self, body: &'a str, opening: Option<usize>) -> &'a str {
        let value = if let Some(opening) = opening {
            self.quote = Some(char::from(body.as_bytes()[opening]));
            &body[opening + 1..]
        } else {
            body
        };
        for (index, character) in value.char_indices() {
            if self.escaped {
                self.escaped = false;
            } else if Some(character) == self.quote {
                self.quote = None;
                return &value[index + character.len_utf8()..];
            } else if character == '\\' && self.quote != Some('\'') {
                self.escaped = true;
            }
        }
        ""
    }
}

fn split_trailing_whitespace(value: &str) -> (&str, &str) {
    let split = value
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index))
        .unwrap_or(value.len());
    value.split_at(split)
}
