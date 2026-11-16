use std::borrow::Cow;

use sage_acp_protocol::contract::ContractError;

#[path = "command.rs"]
pub(crate) mod command;
#[path = "credentials.rs"]
pub(crate) mod credentials;
#[path = "embedded.rs"]
mod embedded;
#[path = "home.rs"]
pub(crate) mod home;
#[path = "locations.rs"]
mod locations;
#[path = "paths.rs"]
pub(crate) mod paths;
#[path = "pem.rs"]
pub(crate) mod pem;
#[path = "prose.rs"]
mod prose;
#[path = "reply_stream.rs"]
mod reply_stream;
#[path = "scalars.rs"]
mod scalars;
#[path = "secrets.rs"]
pub(crate) mod secrets;
#[path = "shell.rs"]
pub(crate) mod shell;
#[path = "spans.rs"]
pub(crate) mod spans;
#[path = "tokens.rs"]
mod tokens;
#[path = "values.rs"]
pub(crate) mod values;

pub(crate) use paths::PathMode as Mode;
use prose::ProsePaths;
pub(crate) use reply_stream::ReplyStream;

pub(crate) fn redact(
    field: &'static str,
    input: &str,
    maximum: usize,
    mode: Mode,
) -> Result<String, ContractError> {
    redact_with(field, input, maximum, mode, ProsePaths::Shown)
}

/// A harness's reply, thought or notice, redacted the same way for every
/// harness. It takes the whole text assembled so far, never one streamed
/// delta: a secret, a path or the home folder may straddle deltas.
pub(crate) fn redact_reply(
    field: &'static str,
    input: &str,
    maximum: usize,
) -> Result<String, ContractError> {
    redact(field, &neutralized(input, false), maximum, Mode::Prose)
}

/// [`redact_reply`] with room for every redaction marker, failing closed to a
/// single marker for text that cannot be redacted.
pub(crate) fn reply_text(input: &str) -> String {
    const REDACTED: &str = "[redacted]";
    let maximum = input
        .len()
        .saturating_mul(sage_core::limits::COMMAND_OUTPUT_REDACTION_BYTES)
        .max(REDACTED.len());
    redact_reply("text", input, maximum).unwrap_or_else(|_| REDACTED.to_owned())
}

/// Prose that keeps absolute paths private as command output does, such as
/// tool content or an id that must carry nothing worth redacting.
pub(crate) fn redact_prose_hiding_paths(
    field: &'static str,
    input: &str,
    maximum: usize,
) -> Result<String, ContractError> {
    redact_with(field, input, maximum, Mode::Prose, ProsePaths::Redacted)
}

fn redact_with(
    field: &'static str,
    input: &str,
    maximum: usize,
    mode: Mode,
    path_display: ProsePaths,
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
        embedded::redact(input, maximum, mode, path_display, field)?
    };
    let input = embedded.as_ref();
    match mode {
        Mode::Prose => prose::redact_prose(field, input, maximum, path_display),
        Mode::CommandOutput | Mode::Permission => command::redact(field, input, maximum, mode),
        Mode::Diagnostic => {
            let output = super::diagnostic::render(input);
            let mut bounded = String::new();
            secrets::append(&mut bounded, &output, maximum, field)?;
            Ok(bounded)
        }
    }
}

/// Whether a character ends a token as whitespace does. Besides whitespace,
/// including the no-break and ideographic spaces, these are the punctuation of
/// CJK and full-width text, such as `，`, `。` or `「`, and general punctuation
/// such as `—` or `“`, so a key, flag or credential written straight after one
/// is read as starting a token.
pub(crate) fn breaks_a_token(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '\u{2010}'..='\u{205e}' | '\u{3000}'..='\u{303f}' | '\u{fe10}'..='\u{fe4f}'
        )
        || (matches!(character, '\u{ff00}'..='\u{ff65}') && !character.is_alphanumeric())
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
