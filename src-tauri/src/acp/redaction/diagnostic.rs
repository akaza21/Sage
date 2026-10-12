const QUOTE_SCAN_LIMIT: usize = 1024;

#[path = "diagnostic/json.rs"]
mod json;

use super::text::{
    credentials::{
        attached_short_value, auth_scheme, contains_token, is_sensitive_prose_flag,
        sensitive_assignment, sensitive_key, url_userinfo,
    },
    paths::{PathKind, PathMode, absolute_path, home_path},
    pem::{redact_pem, scan_window},
};

enum QuoteScan {
    Value(usize),
    TokenOnly,
    Exhausted,
}

pub(crate) fn sanitize_diagnostic_text(value: &str) -> String {
    super::text::redact(
        "diagnostic",
        value,
        value.len().saturating_mul(10),
        PathMode::Diagnostic,
    )
    .unwrap_or_else(|_| "[redacted]".to_owned())
}

pub(super) fn render(value: &str) -> String {
    sanitize_words(value, 2)
}

fn sanitize_words(value: &str, json_depth: u8) -> String {
    let value = redact_pem(value);
    let value = value.as_ref();
    let mut output = String::with_capacity(value.len());
    // redact_next consumes the value after a key, separator, or auth scheme.
    // separator_key permits a standalone ':'/'=' after a sensitive key.
    // Each iteration resets both flags from the consumed token; quoted values
    // and path continuations advance remaining as one bounded unit.
    let mut redact_next = false;
    let mut separator_key = false;
    let mut separator_colon_key = false;
    let mut remaining = value;
    let mut word_end = 0;
    let mut json_tail = false;
    while !remaining.is_empty() {
        let trimmed = remaining.trim_start_matches(char::is_whitespace);
        output.push_str(&remaining[..remaining.len() - trimmed.len()]);
        remaining = trimmed;
        if remaining.is_empty() {
            break;
        }
        let position = value.len() - remaining.len();
        if position >= word_end {
            word_end = position + word_len(remaining);
        }
        let token_len = word_end - position;
        let token = &remaining[..token_len];
        let (prefix, bare, suffix) = diagnostic_word(token);
        if separator_key && (bare == "=" || (bare == ":" && separator_colon_key)) {
            output.push_str(token);
            redact_next = true;
            separator_key = false;
            remaining = &remaining[token_len..];
            continue;
        }
        let assignment = assignment_start(token, prefix, bare, suffix);
        let value_start = assignment.map(|(start, _)| start);
        if json_depth > 0
            && !redact_next
            && value_start.is_none()
            && let Some((text, consumed)) = json::sanitize_word(remaining, token_len, json_depth)
        {
            output.push_str(&text);
            remaining = &remaining[consumed..];
            separator_key = false;
            json_tail = true;
            continue;
        }
        let value_start = value_start.or_else(|| {
            bare.split_once('=')
                .filter(|(key, value)| {
                    sensitive_assignment(key, '=', value)
                        && !value.is_empty()
                        && !value.starts_with('=')
                })
                .map(|(key, _)| prefix.len() + key.len() + 1)
        });
        let lower = if redact_next || value_start.is_some() {
            String::new()
        } else {
            bare.to_ascii_lowercase()
        };
        let secret_prefix =
            if value_start.is_some_and(|start| token[..start - 1].ends_with(['"', '\''])) {
                prefix.trim_end_matches(['"', '\''])
            } else {
                prefix
            };
        let scheme = bare.rsplit([':', '=']).next().is_some_and(auth_scheme);
        let mut consumed = token_len;
        let mut next = false;
        if matches!(bare, "redacted" | "path") && prefix.ends_with('[') && suffix.starts_with(']') {
            output.push_str(token);
        } else if redact_next || value_start.is_some() {
            let start = value_start.unwrap_or_default();
            let empty = assignment.is_some_and(|(_, empty)| empty);
            let quoted = scan_quoted_value(&remaining[start..]);
            if !redact_next && empty {
                if bare
                    .trim_end_matches(':')
                    .trim_matches(['"', '\''])
                    .eq_ignore_ascii_case("bearer")
                {
                    output.push_str(prefix);
                    output.push_str("[redacted]");
                    output.push_str(suffix);
                } else {
                    output.push_str(token);
                }
                next = true;
            } else if remaining[start..].starts_with(['{', '['])
                && !remaining[start..].starts_with("[redacted]")
            {
                if !redact_next {
                    output.push_str(secret_prefix);
                }
                output.push_str("[redacted]");
                consumed = start
                    + json::container_end(&remaining[start..], 0)
                        .unwrap_or(remaining.len() - start);
            } else if matches!(&quoted, QuoteScan::Exhausted) {
                if !redact_next {
                    output.push_str(secret_prefix);
                }
                output.push_str("[redacted]");
                consumed = remaining.len();
            } else if let QuoteScan::Value(length) = quoted {
                if !redact_next {
                    output.push_str(secret_prefix);
                }
                output.push_str("[redacted]");
                consumed = start + length;
            } else if json_tail
                && value_start.is_some()
                && let Some(index) = json::scalar_tail(&remaining[start..], token_len - start)
            {
                output.push_str(secret_prefix);
                output.push_str("[redacted]");
                consumed = start + index;
            } else {
                if !redact_next || !token.starts_with(['"', '\'']) {
                    output.push_str(secret_prefix);
                }
                output.push_str("[redacted]");
                output.push_str(suffix);
                next = scheme || start == token.len() || (redact_next && bare == ":");
            }
        } else if token
            .strip_prefix("[redacted]")
            .is_some_and(|tail| tail.starts_with(char::is_alphanumeric))
        {
            output.push_str("[redacted]");
        } else if attached_short_value(bare)
            && let Some(length) = super::text::values::attached_flag_end(
                &remaining[prefix.len()..],
                PathMode::Diagnostic,
            )
        {
            output.push_str(prefix);
            output.push_str("[redacted]");
            consumed = prefix.len() + length;
            let (_, _, ending) = diagnostic_word(&remaining[..consumed]);
            output.push_str(ending);
        } else if contains_token(&lower) || attached_short_value(bare) {
            output.push_str(prefix);
            output.push_str("[redacted]");
            output.push_str(suffix);
        } else if let Some((start, end)) = url_userinfo(bare) {
            output.push_str(prefix);
            output.push_str(&bare[..start]);
            output.push_str("[redacted]");
            output.push_str(&bare[end..]);
            output.push_str(suffix);
        } else if let Some(kind) = absolute_path(bare, PathMode::Diagnostic) {
            output.push_str(prefix);
            output.push_str("[path]");
            consumed = if kind == PathKind::Windows || home_path(bare) {
                windows_path_len(remaining, token_len)
            } else {
                token_len
            };
            if consumed == token_len {
                output.push_str(&bare[bare.trim_end_matches(':').len()..]);
                output.push_str(suffix);
            } else {
                let (_, tail, ending) = diagnostic_word(&remaining[..consumed]);
                output.push_str(&tail[tail.trim_end_matches(':').len()..]);
                output.push_str(ending);
            }
        } else {
            output.push_str(token);
            next = auth_scheme(&lower) || is_sensitive_prose_flag(bare);
        }
        separator_key = sensitive_key(&lower);
        separator_colon_key = sensitive_assignment(lower.trim_end_matches(':'), ':', "value");
        redact_next = next;
        remaining = &remaining[consumed..];
        json_tail = false;
    }
    output
}

fn word_len(value: &str) -> usize {
    value.find(char::is_whitespace).unwrap_or(value.len())
}

fn assignment_start(token: &str, prefix: &str, bare: &str, suffix: &str) -> Option<(usize, bool)> {
    let body = &token[prefix.len()..];
    let separator = body.find([':', '='])?;
    let first_key = body[..separator].trim_matches(['"', '\'']);
    let colon_assignment = body.as_bytes()[separator] == b':'
        && (body[..separator].ends_with('"') || sensitive_key(first_key) || auth_scheme(first_key));
    if !colon_assignment && let Some(index) = body.find('=') {
        let key = body[..index]
            .rsplit(['?', '&'])
            .next()
            .unwrap_or_default()
            .trim_matches(['"', '\'']);
        let value = &body[index + 1..];
        if sensitive_assignment(key, '=', value)
            && key.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '_' | '-' | '.' | ':' | '[' | ']')
            })
            && !value.starts_with('=')
        {
            if bare.ends_with('=') && prefix.contains(['"', '\'']) && suffix.contains(['"', '\'']) {
                return None;
            }
            return Some((prefix.len() + index + 1, false));
        }
    }
    let index = body.find(':')?;
    if body[..index].ends_with('"') {
        return None;
    }
    let key = body[..index].trim_matches(['"', '\'']).to_ascii_lowercase();
    if !sensitive_assignment(&key, ':', &body[index + 1..]) && !auth_scheme(&key) {
        return None;
    }
    let empty = body[index + 1..]
        .trim_matches(['"', '\'', '}', ']'])
        .is_empty();
    if empty
        && !body[..index].ends_with(['"', '\''])
        && prefix.contains(['"', '\''])
        && suffix.contains(['"', '\''])
    {
        return None;
    }
    Some((prefix.len() + index + 1, empty))
}

fn diagnostic_word(token: &str) -> (&str, &str, &str) {
    let bare_start = token.trim_start_matches(['"', '\'', '`', '(', '[', '{', '<', ',', ';']);
    let prefix_len = token.len() - bare_start.len();
    let mut bare = bare_start.trim_end_matches(['"', '\'', '`', ')', ']', '}', '>', ',', ';', '.']);
    let drive_root =
        bare.len() == 3 && bare.as_bytes()[0].is_ascii_alphabetic() && bare.as_bytes()[1] == b':';
    if !drive_root && bare.ends_with('\\') && bare_start[bare.len()..].starts_with('"') {
        bare = &bare[..bare.len() - 1];
    }
    (&token[..prefix_len], bare, &bare_start[bare.len()..])
}

fn scan_quoted_value(value: &str) -> QuoteScan {
    let Some(quote) = value.chars().next() else {
        return QuoteScan::TokenOnly;
    };
    if !matches!(quote, '"' | '\'') {
        return QuoteScan::TokenOnly;
    }
    let window = scan_window(value);
    if let Some(end) = super::text::spans::quote_end(
        value,
        1,
        quote as u8,
        false,
        window.len(),
        PathMode::Diagnostic,
    ) {
        let boundary = end + word_len(&window[end..]);
        if boundary == window.len()
            && window.len() < value.len()
            && !value[boundary..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            return QuoteScan::Exhausted;
        }
        return QuoteScan::Value(
            end + window[end..boundary]
                .trim_end_matches(['"', '\'', '\x60', ')', ']', '}', '>', ',', ';', '.'])
                .len(),
        );
    }
    if window.len() < value.len() {
        QuoteScan::Exhausted
    } else if quote == '\'' {
        QuoteScan::Value(value.len())
    } else {
        QuoteScan::TokenOnly
    }
}

fn windows_path_len(value: &str, token_len: usize) -> usize {
    let suffix = &value[token_len..];
    let trimmed_suffix = suffix.trim_start_matches(char::is_whitespace);
    let whitespace_len = suffix.len() - trimmed_suffix.len();
    let window = scan_window(trimmed_suffix);
    let mut remaining = window;
    while !remaining.is_empty() {
        let trimmed = remaining.trim_start_matches(char::is_whitespace);
        let length = word_len(trimmed);
        let continuation = &trimmed[..length];
        if continuation.starts_with(['"', '\''])
            || absolute_path(continuation, PathMode::Diagnostic).is_some()
        {
            return token_len;
        }
        let boundary = window.len() - trimmed.len() + length;
        if boundary == window.len()
            && window.len() < trimmed_suffix.len()
            && (length == 0
                || !trimmed_suffix[boundary..]
                    .chars()
                    .next()
                    .is_some_and(char::is_whitespace))
        {
            let word_start = window.len() - trimmed.len();
            let full = &trimmed_suffix[word_start..];
            let full_length = word_len(full);
            let full_word = &full[..full_length];
            return if !full_word.starts_with('\\') && full_word.contains(['\\', '/']) {
                token_len + whitespace_len + word_start + full_length
            } else {
                token_len
            };
        } else if continuation.starts_with('\\') {
            return token_len;
        } else if continuation.contains(['\\', '/']) {
            return token_len + whitespace_len + boundary;
        }
        if length == 0 {
            break;
        }
        remaining = &trimmed[length..];
    }
    token_len
}

#[cfg(test)]
#[path = "diagnostic/tests.rs"]
mod tests;
