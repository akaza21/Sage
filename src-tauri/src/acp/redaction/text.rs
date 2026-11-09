use std::borrow::Cow;

use crate::acp::contract::ContractError;

#[path = "command.rs"]
pub(crate) mod command;
#[path = "credentials.rs"]
pub(crate) mod credentials;
#[path = "embedded.rs"]
mod embedded;
#[path = "paths.rs"]
pub(crate) mod paths;
#[path = "pem.rs"]
pub(crate) mod pem;
#[path = "prose.rs"]
mod prose;
#[path = "scalars.rs"]
mod scalars;
#[path = "secrets.rs"]
pub(crate) mod secrets;
#[path = "shell.rs"]
pub(crate) mod shell;
#[path = "spans.rs"]
pub(crate) mod spans;
#[path = "values.rs"]
pub(crate) mod values;

pub(crate) use paths::PathMode as Mode;

pub(crate) fn redact(
    field: &'static str,
    input: &str,
    maximum: usize,
    mode: Mode,
) -> Result<String, ContractError> {
    if mode == Mode::Permission {
        let tokens = shell::shell_tokens(input).map_err(|_| ContractError::ForbiddenContent {
            field: "permissionCommand",
        })?;
        if tokens.iter().any(|token| {
            paths::absolute_path(&token.cooked, mode).is_some()
                || paths::absolute_path(&input[token.start..token.end], mode).is_some()
        }) {
            return Err(ContractError::ForbiddenContent {
                field: "permissionCommand",
            });
        }
    }
    let embedded = if mode == Mode::Diagnostic {
        std::borrow::Cow::Borrowed(input)
    } else {
        embedded::redact(input, maximum, mode, field)?
    };
    let input = embedded.as_ref();
    match mode {
        Mode::Prose => prose::redact_prose(field, input, maximum),
        Mode::CommandOutput | Mode::Permission => command::redact(field, input, maximum, mode),
        Mode::Diagnostic => {
            let output = super::diagnostic::render(input);
            let mut bounded = String::new();
            secrets::append(&mut bounded, &output, maximum, field)?;
            Ok(bounded)
        }
    }
}

/// Clips display text to `max` bytes at a character boundary, ending a clipped
/// value with an ellipsis.
pub(crate) fn clip_display(value: &str, max: usize) -> String {
    const CLIPPED: &str = "\u{2026}";
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max.saturating_sub(CLIPPED.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{CLIPPED}", &value[..end])
}

/// Replaces control characters (other than tab and line breaks) with a
/// space and, when `strip_format` is set, drops bidi, zero-width and other
/// invisible format characters, so harness text cannot reorder or hide
/// what the user sees.
pub(crate) fn neutralized(input: &str, strip_format: bool) -> Cow<'_, str> {
    if !input.chars().any(|character| {
        (strip_format && is_stripped_format(character)) || is_neutralized_control(character)
    }) {
        return Cow::Borrowed(input);
    }
    let mut output = String::with_capacity(input.len());
    for character in input.chars() {
        if strip_format && is_stripped_format(character) {
            continue;
        }
        output.push(if is_neutralized_control(character) {
            ' '
        } else {
            character
        });
    }
    Cow::Owned(output)
}

fn is_neutralized_control(character: char) -> bool {
    matches!(character, '\u{1}'..='\u{1f}' | '\u{7f}'..='\u{9f}')
        && !matches!(character, '\t' | '\n' | '\r')
}

fn is_stripped_format(character: char) -> bool {
    matches!(
        character,
        '\u{ad}'
            | '\u{61c}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
    )
}

#[cfg(test)]
#[path = "corpus.rs"]
mod corpus;

#[cfg(test)]
#[path = "regression_tests.rs"]
mod regression_tests;
