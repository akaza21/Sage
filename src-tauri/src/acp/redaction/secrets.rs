use super::credentials::{
    assigned_auth_marker, auth_marker, is_sensitive_prose_flag, is_sensitive_prose_token,
    sensitive_assignment, sensitive_header, sensitive_key,
};
use sage_acp_protocol::contract::ContractError;

pub(crate) const REDACTED: &str = "[redacted]";

#[derive(Default)]
pub(crate) struct SecretContext {
    next_value: bool,
    authorization_line: bool,
    key_waiting: bool,
    colon_key_waiting: bool,
}

impl SecretContext {
    pub(crate) fn next_value(&self) -> bool {
        self.next_value
    }

    pub(crate) fn protects_value(&self) -> bool {
        self.next_value || self.authorization_line
    }

    pub(crate) fn quoted_value_start(&self, body: &str) -> Option<usize> {
        if super::credentials::quoted_assignment_syntax(body) {
            return None;
        }
        if (self.next_value || self.authorization_line) && body.starts_with(['\'', '"']) {
            return Some(0);
        }
        let offset = if body.starts_with("$env:") { 5 } else { 0 };
        let separator = body[offset..].find(['=', ':'])? + offset;
        if !(self.next_value || self.authorization_line || sensitive_key(&body[..separator])) {
            return None;
        }
        body[separator + 1..]
            .starts_with(['\'', '"'])
            .then_some(separator + 1)
    }

    pub(crate) fn finish_value(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn redact(&mut self, body: &str, whitespace: &str, continued: bool) -> bool {
        if super::credentials::quoted_assignment_syntax(body) {
            return false;
        }
        let separator = self.next_value && !continued && matches!(body, "<" | ":" | "=");
        let was_authorization_line = self.authorization_line;
        let literal_value = continued || self.next_value;
        let structural_flag = self.next_value
            && !continued
            && !body.starts_with(['\'', '"', '`'])
            && is_sensitive_prose_flag(body);
        let header = sensitive_header(body, super::Mode::CommandOutput);
        let redacted = !body.is_empty()
            && !separator
            && (self.authorization_line
                || literal_value
                || (header
                    && !body.eq_ignore_ascii_case("authorization:")
                    && !body.eq_ignore_ascii_case("proxy-authorization:"))
                || is_sensitive_prose_token(body));
        if !body.is_empty() {
            let token = body.trim_matches(|character: char| {
                matches!(character, '\'' | '"' | '`' | '{' | '}' | ',' | ';')
            });
            let separated_assignment = token
                .split_once('=')
                .or_else(|| token.split_once(':'))
                .is_some_and(|(key, value)| {
                    sensitive_assignment(key, if token.contains('=') { '=' } else { ':' }, value)
                        && value.trim_matches(['\'', '"', '`']).is_empty()
                });
            self.next_value = structural_flag
                || separator
                || (!literal_value
                    && (auth_marker(body, super::Mode::CommandOutput)
                        || is_sensitive_prose_flag(body)
                        || separated_assignment
                        || (self.key_waiting
                            && (token == "=" || (token == ":" && self.colon_key_waiting)))
                        || assigned_auth_marker(token, super::Mode::CommandOutput)));
            self.key_waiting = !literal_value && sensitive_key(token.trim_end_matches(':'));
            self.colon_key_waiting =
                sensitive_assignment(token.trim_end_matches(':'), ':', "value");
            if !literal_value && header {
                self.authorization_line = true;
            }
        }
        if whitespace.contains(['\n', '\r']) {
            self.authorization_line = false;
            if was_authorization_line || header {
                self.next_value = false;
                self.key_waiting = false;
            }
        }
        redacted
    }
}

pub(crate) fn append(
    output: &mut String,
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), ContractError> {
    let invalid_length = || ContractError::InvalidLength {
        field,
        minimum: 0,
        maximum,
    };
    let next = output
        .len()
        .checked_add(value.len())
        .ok_or_else(invalid_length)?;
    if next > maximum {
        return Err(invalid_length());
    }
    output
        .try_reserve(value.len())
        .map_err(|_| invalid_length())?;
    output.push_str(value);
    Ok(())
}
