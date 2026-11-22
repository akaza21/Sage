use std::{borrow::Cow, sync::LazyLock};

use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;

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
    Ok(redact_reply_settling(field, input, maximum)?.text)
}

/// Approval arguments must keep every nonsecret operation visible. Reuse the
/// same credential scanner as shell approvals, refusing any exhausted scan
/// (including inside embedded strings) instead of approving a partial display.
pub(crate) fn redact_complete_arguments(
    field: &'static str,
    input: &str,
    maximum: usize,
) -> Result<String, ContractError> {
    if input.len() > maximum {
        return Err(ContractError::ForbiddenContent { field });
    }
    let embedded = embedded::redact(input, maximum, Mode::Prose, ProsePaths::Shown, field, true)?;
    let redacted = prose::redact_prose_settling(field, &embedded, maximum, ProsePaths::Shown)?;
    if redacted.truncated {
        return Err(ContractError::ForbiddenContent { field });
    }
    Ok(redacted.text)
}

fn redact_reply_settling(
    field: &'static str,
    input: &str,
    maximum: usize,
) -> Result<prose::RedactedProse, ContractError> {
    let input = neutralized(input, false);
    let embedded = embedded::redact(
        &input,
        maximum,
        Mode::Prose,
        ProsePaths::Shown,
        field,
        false,
    )?;
    prose::redact_prose_settling(field, &embedded, maximum, ProsePaths::Shown)
}

/// [`redact_reply`] with room for every redaction marker, failing closed to a
/// single marker for text that cannot be redacted.
pub(crate) fn reply_text(input: &str) -> String {
    reply_text_settling(input).text
}

/// [`reply_text`], and whether nothing at its end waits for text after it.
pub(crate) fn reply_text_settling(input: &str) -> prose::RedactedProse {
    const REDACTED: &str = "[redacted]";
    let maximum = input
        .len()
        .saturating_mul(sage_core::limits::COMMAND_OUTPUT_REDACTION_BYTES)
        .max(REDACTED.len());
    redact_reply_settling("text", input, maximum).unwrap_or_else(|_| prose::RedactedProse {
        text: REDACTED.to_owned(),
        settled: false,
        truncated: true,
    })
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
        embedded::redact(input, maximum, mode, path_display, field, false)?
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

/// Whether a character ends a token as whitespace does: whitespace, or any
/// Unicode punctuation outside ASCII, such as `，`, `「` or `—`, which CJK
/// prose puts no space after. ASCII punctuation stays inside a token, since it
/// spells keys, flags, paths and secrets.
pub(crate) fn breaks_a_token(character: char) -> bool {
    #[cfg(test)]
    if spaces_only() {
        return character.is_whitespace();
    }
    static PUNCTUATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\p{P}$").expect("valid punctuation class"));
    character.is_whitespace()
        || (!character.is_ascii()
            && !character.is_alphanumeric()
            && PUNCTUATION.is_match(character.encode_utf8(&mut [0; 4])))
}

/// Characters that join a token into a secret or a path. A token holding one
/// ends only at whitespace, since a secret or a file name may hold any
/// punctuation or script.
pub(crate) const TOKEN_JOINERS: [char; 5] = [':', '=', '@', '/', '\\'];

/// Most bytes read ahead for the key a token may start, as `password—backup`
/// in `{"password—backup": …}`.
const KEY_SCAN_BYTES: usize = 256;

/// The token at the start of `text`, and the character that ended it, if any.
///
/// Whitespace ends a token. Until the token holds a [`TOKEN_JOINERS`]
/// character, so does a character that [`breaks_a_token`] or a
/// [`script_change`], so a key or flag may start there, unless that would cut
/// the key of an assignment or a sensitive key (see [`Key::cut_at`]). A `value` the redactor waits for, such as the
/// one after `--password` or `Bearer`, ends only at whitespace: its characters
/// never split it.
pub(crate) fn next_token(text: &str, value: bool) -> (&str, &str) {
    let mut joined = value;
    let mut previous = None;
    let mut key = None;
    for (index, character) in text.char_indices() {
        let next = index + character.len_utf8();
        if character.is_whitespace() {
            return (&text[..index], &text[index..next]);
        }
        let split = if joined {
            None
        } else if breaks_a_token(character) {
            Some(next)
        } else if previous.is_some_and(|before| script_change(before, character)) {
            Some(index)
        } else {
            None
        };
        if let Some(after) = split
            && !key
                .get_or_insert_with(|| Key::ahead(text))
                .cut_at(text, after)
        {
            return (&text[..index], &text[index..after]);
        }
        joined |= TOKEN_JOINERS.contains(&character);
        previous = Some(character);
    }
    (text, "")
}

/// The key a token may start: the text up to its first [`TOKEN_JOINERS`]
/// character or whitespace, read at most [`KEY_SCAN_BYTES`] ahead.
struct Key {
    /// `None` when the key runs past the bytes read ahead.
    end: Option<usize>,
    assigned: bool,
    /// Set once a key with no known end might be cut.
    closed: bool,
}

impl Key {
    fn ahead(text: &str) -> Self {
        let mut scan = text.len().min(KEY_SCAN_BYTES);
        while !text.is_char_boundary(scan) {
            scan -= 1;
        }
        let window = &text[..scan];
        let end = window
            .find(|character: char| character.is_whitespace() || TOKEN_JOINERS.contains(&character))
            .or((scan == text.len()).then_some(scan));
        Self {
            end,
            assigned: end.is_some_and(|end| {
                window[end..].starts_with(['=', ':']) && !window[end..].starts_with("://")
            }),
            closed: false,
        }
    }

    /// Whether splitting at `at` cuts a key: the key of an assignment, or a
    /// sensitive key, splits only into a part that reads as a sensitive key
    /// itself, outside the quotes the key is written in and, for a flag such
    /// as `--old—token`, as a sensitive flag too. A key running past the
    /// bytes read ahead has no known end, so once the text before `at` reads
    /// as a sensitive key, a flag or a quoted key, the token is never split.
    fn cut_at(&mut self, text: &str, at: usize) -> bool {
        let Some(end) = self.end else {
            let before = &text[..at];
            self.closed = self.closed
                || before.starts_with('-')
                || before.contains(['"', '\''])
                || credentials::sensitive_key(before);
            return self.closed;
        };
        if at > end {
            return false;
        }
        let (key, part) = (&text[..end], &text[at..end]);
        (self.assigned || credentials::sensitive_key(key))
            && ((key.ends_with(['"', '\'']) && key[..at].contains(['"', '\'']))
                || !credentials::sensitive_key(part)
                || (credentials::is_sensitive_prose_flag(key)
                    && !credentials::is_sensitive_prose_flag(part)))
    }
}

/// Whether a word boundary falls between `before` and `after` where ASCII
/// meets a letter or digit that UAX #29 never joins into a word with a Latin
/// letter, such as an ideograph or kana.
fn script_change(before: char, after: char) -> bool {
    #[cfg(test)]
    if spaces_only() {
        return false;
    }
    let apart_from_latin = |character: char| {
        character.is_alphanumeric() && format!("{character}a").split_word_bounds().nth(1).is_some()
    };
    match (before.is_ascii(), after.is_ascii()) {
        (false, true) => apart_from_latin(before),
        (true, false) => apart_from_latin(after),
        _ => false,
    }
}

#[cfg(test)]
thread_local! {
    static SPACES_ONLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn spaces_only() -> bool {
    SPACES_ONLY.get()
}

/// Runs `redact` with tokens split at whitespace alone, as they were before
/// punctuation and scripts could start one, to compare against.
#[cfg(test)]
pub(crate) fn with_spaces_only<T>(redact: impl FnOnce() -> T) -> T {
    SPACES_ONLY.set(true);
    let result = redact();
    SPACES_ONLY.set(false);
    result
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
