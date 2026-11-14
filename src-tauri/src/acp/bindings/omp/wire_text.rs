use super::WireError;
use crate::acp::{
    contract::SafeText,
    redaction::text::{self, Mode},
};
use sage_core::limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS};

pub(crate) fn normalized_text<const MAX: usize>(
    field: &'static str,
    text: &str,
) -> Result<SafeText<MAX>, WireError> {
    if text.len() > MAX || text.contains('\0') {
        return Err(WireError::TextLimit);
    }
    let result = text::redact(field, text, MAX, Mode::Prose).map_err(|_| WireError::TextLimit)?;
    SafeText::new(field, result).map_err(|_| WireError::TextLimit)
}

/// General `normalized_text` callers retain their strict `SafeText` maximum; oversized command
/// output is valid and is capped later.
pub(crate) fn sanitized_command_output(input: &str) -> Result<String, WireError> {
    let maximum_input = LIMITS.wire.frame_bytes as usize;
    let maximum_output = maximum_input.saturating_mul(COMMAND_OUTPUT_REDACTION_BYTES);
    if input.len() > maximum_input || input.contains('\0') {
        return Err(WireError::TextLimit);
    }

    text::redact("toolOutput", input, maximum_output, Mode::CommandOutput)
        .map_err(|_| WireError::TextLimit)
}

pub(super) fn valid_opaque_id(value: &str) -> bool {
    use text::{
        credentials::{
            auth_scheme, contains_token, inline_auth, is_sensitive_prose_flag,
            sensitive_assignment, url_userinfo,
        },
        paths::absolute_path,
    };
    if value.is_empty()
        || value.len() as u64 > LIMITS.wire.external_id_bytes
        || value.contains('\0')
    {
        return false;
    }
    let mut tokens = value
        .split_whitespace()
        .flat_map(|token| token.split(['?', '&']))
        .peekable();
    while let Some(token) = tokens.next() {
        let assignment = token
            .split_once('=')
            .is_some_and(|(key, value)| !value.is_empty() && sensitive_assignment(key, '=', value))
            || token.split_once(':').is_some_and(|(key, value)| {
                !key.eq_ignore_ascii_case("session")
                    && !value.is_empty()
                    && sensitive_assignment(key, ':', value)
            });
        let credential_context = tokens.peek().is_some()
            && (auth_scheme(token.trim_end_matches(':')) || is_sensitive_prose_flag(token));
        if assignment
            || credential_context
            || inline_auth(token)
            || contains_token(token)
            || url_userinfo(token).is_some()
            || absolute_path(token, Mode::Permission).is_some()
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_ids_allow_protocol_names_not_prose_heuristics() {
        for input in [
            "toolu_01Q5Yh6Tz8aN3v9W2b7K4s1D",
            "call_d2e31b70-1905-4c26-a2cf-28c78cdef564",
            "session:42a6de0a-bc19-4fea-9036-3ac79cebb954",
            "omp:session/42a6de0a/tool/17",
            "message/turn/01JNZ9HF37MPTAG4M5YTH7C8QX",
            "-user-action",
            "-pending-tool",
            "tokens/1234",
        ] {
            assert!(valid_opaque_id(input), "{input}");
        }
        for input in [
            "",
            "\0",
            "password=canary",
            "TOKEN2=abc",
            "Bearer canary",
            "password:canary",
            "token:canary",
            "to_ken=canary",
            "t_o_k_e_n=canary",
            "--token canary",
            "x=1&password=canary",
            "https://host/?x=1&password=canary",
            "https://user:canary@host/",
            "sk-abc",
            "ASIAIOSFODNN7EXAMPLE",
            "/home/alice",
            r"C:\Users\Jane",
            r"\\host\share",
            "file:///home/alice/x",
        ] {
            assert!(!valid_opaque_id(input), "{input}");
        }
        assert!(!valid_opaque_id(&"x".repeat(513)));
    }

    #[test]
    fn output_and_prose_use_shared_credential_and_path_detection() {
        for (input, secret) in [
            ("before --token abc after", "abc"),
            ("before TOKEN='private value' after", "private"),
            ("before >/home/alice/x after", "alice"),
            (r"before -isystemC:\Users\Jane\inc after", "Jane"),
            ("before C:/Users/Jane Doe/x after", "Doe"),
            (r#"before password={"a":"b","c":"d"} after"#, r#""c":"d""#),
        ] {
            for actual in [
                sanitized_command_output(input).unwrap(),
                normalized_text::<512>("text", input)
                    .unwrap()
                    .as_str()
                    .to_owned(),
            ] {
                assert!(!actual.contains(secret), "{input}: {actual}");
                assert!(
                    actual.starts_with("before ") && actual.ends_with(" after"),
                    "{input}: {actual}"
                );
            }
        }
    }

    #[test]
    fn admission_and_redaction_expansion_remain_bounded() {
        assert!(normalized_text::<8>("text", "--token a").is_err());
        assert!(normalized_text::<8>("text", "\0").is_err());
        assert!(sanitized_command_output("\0").is_err());
        assert!(
            sanitized_command_output(&"a".repeat(LIMITS.wire.frame_bytes as usize + 1)).is_err()
        );
        let input = "p=a ".repeat(20_000);
        let shown = sanitized_command_output(&input).unwrap();
        assert!(!shown.contains("p=a"));
        assert!(shown.len() <= input.len() * COMMAND_OUTPUT_REDACTION_BYTES);
    }
}
