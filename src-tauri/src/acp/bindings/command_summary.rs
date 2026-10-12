use crate::acp::contract::SafeText;

use crate::acp::redaction::text::{
    self, Mode,
    credentials::{
        assigned_auth_marker, attached_short_value, auth_continuation, auth_marker, contains_token,
        inline_auth, is_sensitive_prose_flag, sensitive_assignment, sensitive_flag,
        sensitive_header, sensitive_header_name, sensitive_key, url_userinfo,
    },
    shell::shell_tokens,
};

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

    let redacted = text::redact(
        "permissionCommand",
        &neutral,
        MAX_COMMAND_BYTES,
        Mode::Permission,
    )
    .map_err(|error| match error {
        crate::acp::contract::ContractError::InvalidLength { .. } => CommandSummaryError::TooLong,
        _ => CommandSummaryError::Hidden,
    })?;
    let shown = redact_tokens(&redacted.replace(['\n', '\r'], " "))
        .map_err(|_| CommandSummaryError::Hidden)?;
    SafeText::new("permissionCommand", shown).map_err(|_| CommandSummaryError::TooLong)
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
        let header =
            sensitive_header(&lower, Mode::Permission) || (header_name_waiting && lower == ":");
        let spaced_assignment = sensitive_key_waiting && cooked == "=";
        let auth_marker = auth_marker(&lower, Mode::Permission);
        let inline_auth = inline_auth(&lower);
        let assigned = cooked.split_once('=').is_some_and(|(key, value)| {
            sensitive_assignment(key, '=', value) || sensitive_flag(key)
        });
        let attached = attached_short_value(cooked);
        let separated_flag = is_sensitive_prose_flag(cooked);
        let known_secret = contains_token(cooked) || url_userinfo(cooked).is_some();
        let redact = redact_next || header || assigned || attached || inline_auth || known_secret;
        output.push_str(if redact {
            "[redacted]"
        } else {
            &input[token.start..token.end]
        });
        redact_next = (separated_flag && !assigned)
            || auth_marker
            || (header && lower.ends_with(':'))
            || (assigned && assigned_auth_marker(&lower, Mode::Permission))
            || spaced_assignment;
        // Authorization: Bearer value is three tokens; the scheme is secret
        // context and must carry the redaction through to its value.
        if redact && auth_continuation(&lower, Mode::Permission) {
            redact_next = true;
        }
        header_name_waiting = sensitive_header_name(&lower, Mode::Permission);
        sensitive_key_waiting = sensitive_key(cooked) || sensitive_flag(cooked);
        cursor = token.end;
    }
    output.push_str(&input[cursor..]);
    Ok(output)
}

fn is_hidden_format(ch: char) -> bool {
    matches!(ch,
        '\u{00ad}' | '\u{061c}' | '\u{180e}' | '\u{200b}'..='\u{200f}'
        | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}'
    )
}
