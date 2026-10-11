use std::sync::OnceLock;

use regex::Regex;

use crate::acp::contract::SafeText;

#[path = "command_summary/redaction.rs"]
pub(crate) mod redaction;

#[path = "command_summary/secrets.rs"]
pub(crate) mod secrets;

const MAX_COMMAND_BYTES: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandSummaryError {
    TooLong,
    Undisplayable,
    Hidden,
}

/// A display-only command summary. It never carries authority to execute.
pub(crate) fn sanitize_command_summary(
    command: &str,
) -> Result<SafeText<MAX_COMMAND_BYTES>, CommandSummaryError> {
    if command.len() > MAX_COMMAND_BYTES {
        return Err(CommandSummaryError::TooLong);
    }
    if command.contains('\0') {
        return Err(CommandSummaryError::Undisplayable);
    }
    let neutral: String = command
        .chars()
        .filter_map(|ch| {
            if is_hidden_format(ch) {
                None
            } else if ch.is_control() && !matches!(ch, '\n' | '\r') {
                Some(' ')
            } else {
                Some(ch)
            }
        })
        .collect();

    let tokens = shell_tokens(&neutral).map_err(|_| CommandSummaryError::Hidden)?;
    if tokens
        .iter()
        .any(|token| redaction::contains_absolute_path(&token.cooked, true))
    {
        return Err(CommandSummaryError::Hidden);
    }
    let redacted = redaction::redact("permissionCommand", &neutral, MAX_COMMAND_BYTES, true)
        .map_err(|error| match error {
            crate::acp::contract::ContractError::InvalidLength { .. } => {
                CommandSummaryError::TooLong
            }
            _ => CommandSummaryError::Hidden,
        })?;
    let shown = redact_tokens(&redacted.replace(['\n', '\r'], " "))
        .map_err(|_| CommandSummaryError::Hidden)?;
    SafeText::new("permissionCommand", shown).map_err(|_| CommandSummaryError::TooLong)
}

/// Detect a credential-bearing prose token without interpreting shell quoting.
pub(crate) fn is_sensitive_prose_token(input: &str) -> bool {
    let token = input.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    let lower = token.to_ascii_lowercase();
    let assigned = token
        .split_once('=')
        .or_else(|| token.split_once(':'))
        .is_some_and(|(key, value)| !value.is_empty() && sensitive_key(key));
    let inline_auth = lower
        .split_once(':')
        .is_some_and(|(kind, value)| matches!(kind, "bearer" | "basic") && !value.is_empty());
    assigned
        || inline_auth
        || url_userinfo_regex().is_match(token)
        || standalone_token_regex().is_match(token)
}

pub(crate) fn is_sensitive_prose_flag(input: &str) -> bool {
    let flag = input.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | ',' | ';'));
    flag.starts_with('-') && !flag.contains('=') && (sensitive_flag(flag) || sensitive_key(flag))
}

#[derive(Debug)]
struct ShellToken {
    start: usize,
    end: usize,
    cooked: String,
}

/// Boundaries are shell arguments, not whitespace fragments. Spans let us
/// replace a complete argument without reassembling or exposing quoted tails.
fn shell_tokens(input: &str) -> Result<Vec<ShellToken>, ()> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut cooked = String::new();
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if escaped {
            cooked.push(ch);
            escaped = false;
            continue;
        }
        if quote.is_none()
            && (input[index..].starts_with("<<")
                || input[index..].starts_with("@'")
                || input[index..].starts_with("@\""))
        {
            return Err(());
        }
        if ch == '\\' || ch == '`' {
            start.get_or_insert(index);
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            } else {
                cooked.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            start.get_or_insert(index);
            quote = Some(ch);
        } else if ch.is_whitespace() {
            if let Some(begin) = start.take() {
                tokens.push(ShellToken {
                    start: begin,
                    end: index,
                    cooked: std::mem::take(&mut cooked),
                });
            }
        } else {
            start.get_or_insert(index);
            cooked.push(ch);
        }
    }
    if quote.is_some() || escaped {
        return Err(());
    }
    if let Some(begin) = start {
        tokens.push(ShellToken {
            start: begin,
            end: input.len(),
            cooked,
        });
    }
    Ok(tokens)
}

fn redact_tokens(input: &str) -> Result<String, ()> {
    let tokens = shell_tokens(input)?;
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    let mut redact_next = false;
    let mut header_name_waiting = false;
    let mut sensitive_key_waiting = false;
    for token in tokens {
        output.push_str(&input[cursor..token.start]);
        let cooked = token.cooked.as_str();
        if redact_next && cooked == "<" {
            output.push_str(&input[token.start..token.end]);
            cursor = token.end;
            continue;
        }
        let lower = cooked.to_ascii_lowercase();
        let header = sensitive_header(&lower) || (header_name_waiting && lower == ":");
        let spaced_assignment = sensitive_key_waiting && cooked == "=";
        let auth_marker = matches!(lower.as_str(), "bearer" | "basic" | "bearer:" | "basic:");
        let inline_auth = lower
            .split_once(':')
            .is_some_and(|(kind, value)| matches!(kind, "bearer" | "basic") && !value.is_empty());
        let assigned = cooked
            .split_once('=')
            .is_some_and(|(key, _)| sensitive_key(key) || sensitive_flag(key));
        let attached = cooked.len() > 2
            && (cooked.starts_with("-p")
                || cooked.starts_with("-P")
                || cooked.starts_with("-u")
                || cooked.starts_with("-U"));
        let long_p_flag = matches!(
            lower.as_str(),
            "-password" | "-pass" | "-pw" | "-passwd" | "-pwd"
        );
        let separated_flag = cooked.starts_with('-')
            && (sensitive_key(cooked) || sensitive_flag(cooked))
            && (!attached || long_p_flag);
        let known_secret =
            standalone_token_regex().is_match(cooked) || url_userinfo_regex().is_match(cooked);
        let redact = redact_next || header || assigned || attached || inline_auth || known_secret;
        output.push_str(if redact {
            "[redacted]"
        } else {
            &input[token.start..token.end]
        });
        redact_next = (separated_flag && !assigned)
            || auth_marker
            || (header && lower.ends_with(':'))
            || (assigned && lower.ends_with("=bearer"))
            || spaced_assignment;
        // Authorization: Bearer value is three tokens; the scheme is secret
        // context and must carry the redaction through to its value.
        if redact && matches!(lower.as_str(), "bearer" | "basic" | "token") {
            redact_next = true;
        }
        header_name_waiting = sensitive_header_name(&lower);
        sensitive_key_waiting = sensitive_key(cooked) || sensitive_flag(cooked);
        cursor = token.end;
    }
    output.push_str(&input[cursor..]);
    Ok(output)
}

fn sensitive_header(lower: &str) -> bool {
    [
        "authorization",
        "proxy-authorization",
        "cookie",
        "x-api-key",
        "x-auth-token",
    ]
    .iter()
    .any(|name| {
        lower
            .find(name)
            .is_some_and(|index| lower[index + name.len()..].trim_start().starts_with(':'))
    })
}

fn sensitive_header_name(lower: &str) -> bool {
    matches!(
        lower,
        "authorization" | "proxy-authorization" | "cookie" | "x-api-key" | "x-auth-token"
    )
}

fn sensitive_flag(flag: &str) -> bool {
    matches!(
        flag.to_ascii_lowercase().as_str(),
        "-u" | "--user" | "--proxy-user" | "--userpwd" | "--username" | "--proxy-username"
    )
}

fn sensitive_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    if matches!(upper.trim_start_matches('-'), "P" | "PASS" | "PW") {
        return true;
    }
    [
        "KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "PWD",
        "AUTH",
        "CREDENTIAL",
        "COOKIE",
        "SESSION",
    ]
    .iter()
    .any(|part| upper.contains(part))
}

fn is_hidden_format(ch: char) -> bool {
    matches!(ch,
        '\u{00ad}' | '\u{061c}' | '\u{180e}' | '\u{200b}'..='\u{200f}'
        | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}'
    )
}

fn url_userinfo_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)[a-z][a-z0-9+.-]*://[^\s/@]+@").expect("static URL pattern")
    })
}

fn standalone_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b(?:sk-[a-z0-9_-]{3,}|ghp_[a-z0-9_]{3,}|xox[a-z]-[a-z0-9_-]{3,})\b")
            .expect("static token pattern")
    })
}
