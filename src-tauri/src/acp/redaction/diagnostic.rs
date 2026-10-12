const QUOTE_SCAN_LIMIT: usize = 1024;

#[path = "diagnostic/json.rs"]
mod json;

#[path = "diagnostic/secrets.rs"]
mod secrets;
use secrets::scan_window;

enum QuoteScan {
    Value(usize),
    TokenOnly,
    Exhausted,
}

pub(crate) fn sanitize_diagnostic_text(value: &str) -> String {
    sanitize_words(value, 2)
}

fn sanitize_words(value: &str, json_depth: u8) -> String {
    let value = secrets::redact_pem(value);
    let value = value.as_ref();
    let mut output = String::with_capacity(value.len());
    // redact_next consumes the value after a key, separator, or auth scheme.
    // separator_key permits a standalone ':'/'=' after a sensitive key.
    // Each iteration resets both flags from the consumed token; quoted values
    // and path continuations advance remaining as one bounded unit.
    let mut redact_next = false;
    let mut separator_key = false;
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
        if separator_key && matches!(bare, "=" | ":") {
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
                    !key.is_empty() && !value.is_empty() && !value.starts_with('=')
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
        let scheme = bare.rsplit([':', '=']).next().is_some_and(is_auth_scheme);
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
        } else if secrets::contains_sk_secret(&lower) {
            output.push_str(prefix);
            output.push_str("[redacted]");
            output.push_str(suffix);
        } else if let Some((start, end)) = secrets::url_userinfo(bare) {
            output.push_str(prefix);
            output.push_str(&bare[..start]);
            output.push_str("[redacted]");
            output.push_str(&bare[end..]);
            output.push_str(suffix);
        } else if let Some(windows) = absolute_path(bare) {
            output.push_str(prefix);
            output.push_str("[path]");
            consumed = if windows {
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
            next = lower == "bearer" || (lower.starts_with("--") && sensitive_key(&lower));
        }
        separator_key = sensitive_key(&lower);
        redact_next = next;
        remaining = &remaining[consumed..];
        json_tail = false;
    }
    output
}

fn word_len(value: &str) -> usize {
    value.find(char::is_whitespace).unwrap_or(value.len())
}

fn sensitive_key(key: &str) -> bool {
    let key = key
        .trim_matches(['"', '\'', '`', ':'])
        .trim_start_matches('-');
    let lower = key.to_ascii_lowercase();
    let normalized = lower.trim_start_matches("x-").replace(['-', '_', '.'], "");
    matches!(
        normalized.as_str(),
        "key" | "auth" | "pass" | "pwd" | "awsaccesskeyid"
    ) || [
        "token",
        "secret",
        "secretkey",
        "password",
        "passwd",
        "passphrase",
        "apikey",
        "accesskey",
        "sessionid",
        "jsessionid",
        "privatekey",
        "credential",
        "credentials",
        "authorization",
        "cookie",
    ]
    .iter()
    .any(|suffix| normalized.ends_with(suffix))
        || normalized.contains("secret")
}

fn is_auth_scheme(value: &str) -> bool {
    ["bearer", "basic", "token", "digest", "negotiate"]
        .iter()
        .any(|scheme| value.eq_ignore_ascii_case(scheme))
}

fn assignment_start(token: &str, prefix: &str, bare: &str, suffix: &str) -> Option<(usize, bool)> {
    let body = &token[prefix.len()..];
    let separator = body.find([':', '='])?;
    let first_key = body[..separator].trim_matches(['"', '\'']);
    let colon_assignment = body.as_bytes()[separator] == b':'
        && (body[..separator].ends_with('"')
            || sensitive_key(first_key)
            || first_key.eq_ignore_ascii_case("bearer"));
    if !colon_assignment && let Some(index) = body.find('=') {
        let key = body[..index]
            .rsplit(['?', '&'])
            .next()
            .unwrap_or_default()
            .trim_matches(['"', '\'']);
        let value = &body[index + 1..];
        if !key.is_empty()
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
    if !sensitive_key(&key) && key != "bearer" {
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
    let mut escaped = false;
    for (index, character) in window.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if character == quote {
            let end = index + character.len_utf8();
            let following = value[end..].chars().next();
            if quote == '\'' && following.is_some_and(char::is_alphanumeric) {
                continue;
            }
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
            let tail = &window[end..boundary];
            return QuoteScan::Value(
                end + tail
                    .trim_end_matches(['"', '\'', '`', ')', ']', '}', '>', ',', ';', '.'])
                    .len(),
            );
        } else if quote == '"' && character == '\\' {
            escaped = true;
        }
    }
    if window.len() < value.len() {
        QuoteScan::Exhausted
    } else if quote == '\'' {
        QuoteScan::Value(value.len())
    } else {
        QuoteScan::TokenOnly
    }
}

fn absolute_path(token: &str) -> Option<bool> {
    let lower = token.to_ascii_lowercase();
    if lower.contains("file:///") || lower.contains("file://\\\\") {
        return Some(false);
    }
    let bytes = token.as_bytes();
    for index in 0..bytes.len() {
        if index != 0
            && !matches!(
                bytes[index - 1],
                b'"' | b'\'' | b'`' | b'(' | b'[' | b'{' | b'<' | b'=' | b',' | b';' | b'@' | b':'
            )
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
            return Some(true);
        }
        if rest.starts_with(b"/") && !rest.starts_with(b"//") {
            return Some(false);
        }
    }
    None
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
        if continuation.starts_with(['"', '\'']) || absolute_path(continuation).is_some() {
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
            return if !full_word.starts_with('\\') && full_word.contains('\\') {
                token_len + whitespace_len + word_start + full_length
            } else {
                token_len
            };
        } else if continuation.starts_with('\\') {
            return token_len;
        } else if continuation.contains('\\') {
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
