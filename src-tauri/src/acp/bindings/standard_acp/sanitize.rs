use std::borrow::Cow;

#[path = "sanitize/prose.rs"]
mod prose;
pub(crate) use prose::sanitized_content_output;

use crate::acp::{
    bindings::command_summary::redaction::redact,
    contract::{ContractError, SafeText},
    limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS},
};

pub(crate) fn normalized_text<const MAX: usize>(
    field: &'static str,
    input: &str,
) -> Result<SafeText<MAX>, ContractError> {
    redacted_text(field, input, false)
}

pub(crate) fn normalized_label_text<const MAX: usize>(
    field: &'static str,
    input: &str,
) -> Result<SafeText<MAX>, ContractError> {
    redacted_text(field, input, true)
}

pub(crate) fn neutralized_label_text<const MAX: usize>(
    field: &'static str,
    input: &str,
) -> Result<SafeText<MAX>, ContractError> {
    if input.len() > MAX {
        return SafeText::new(field, input);
    }
    SafeText::new(field, neutralized(input, true).as_ref())
}

fn redacted_text<const MAX: usize>(
    field: &'static str,
    input: &str,
    strip_format: bool,
) -> Result<SafeText<MAX>, ContractError> {
    if input.len() > MAX {
        return SafeText::new(field, input);
    }
    let input = neutralized(input, strip_format);
    let output = redact(field, &input, MAX, false)?;
    SafeText::new(field, output)
}

fn neutralized(input: &str, strip_format: bool) -> Cow<'_, str> {
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

/// Sanitizes one already frame-bounded command-output value before its display
/// suffix is retained. This deliberately has a wider intermediate bound than
/// `SafeText<65536>`: a valid oversized command output must be truncated, not
/// rejected before redaction.
pub(crate) fn sanitized_command_output(input: &str) -> Result<String, ContractError> {
    let maximum_input = LIMITS.wire.frame_bytes as usize;
    let maximum_output = maximum_input.saturating_mul(COMMAND_OUTPUT_REDACTION_BYTES);
    if input.contains('\0') {
        return Err(ContractError::ForbiddenContent {
            field: "toolOutput",
        });
    }
    if input.len() > maximum_input {
        return Err(ContractError::InvalidLength {
            field: "toolOutput",
            minimum: 0,
            maximum: maximum_input,
        });
    }

    let input = neutralized(input, false);
    redact("toolOutput", &input, maximum_output, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::bindings::command_summary::sanitize_command_summary;

    #[test]
    fn retained_commands_and_output_hide_paths_after_shell_boundaries_at_any_position() {
        for prefix in [String::new(), "x".repeat(600)] {
            for boundary in [">", ">>", "2>", "<", "|", ";", "&&", "$(", "`"] {
                for path in [
                    "/home/alice/private.txt",
                    r"C:\Users\Alice\private.txt",
                    r"\\server\private\file.txt",
                ] {
                    let input = format!("echo {prefix}{boundary}{path}");
                    assert!(sanitize_command_summary(&input).is_err(), "{input}");
                    for shown in [
                        normalized_text::<65536>("toolOutput", &input)
                            .unwrap()
                            .as_str()
                            .to_owned(),
                        sanitized_command_output(&input).unwrap(),
                    ] {
                        assert!(!shown.contains(path), "{input}: {shown}");
                        assert!(shown.contains("[redacted]"), "{input}: {shown}");
                    }
                }
            }
        }
    }

    #[test]
    fn retained_commands_and_output_redact_complete_quoted_secret_values() {
        for marker in [
            "--token ",
            "--password ",
            "--secret ",
            "--api-key ",
            "API_KEY=",
            "TOKEN=",
            "SECRET=",
            "PASSWORD=",
            "Authorization: Bearer ",
        ] {
            for value in [
                "'private value'",
                "\"private value\"",
                "'private \"nested\" value'",
                "\"private \\\"nested\\\" value\"",
            ] {
                let input = format!(
                    "echo {} {marker}{value} {marker}{value}\nnext",
                    "x".repeat(600)
                );
                for shown in [
                    sanitize_command_summary(&input)
                        .unwrap()
                        .as_str()
                        .to_owned(),
                    normalized_text::<65536>("toolOutput", &input)
                        .unwrap()
                        .as_str()
                        .to_owned(),
                    sanitized_command_output(&input).unwrap(),
                ] {
                    for secret in ["private", "value", "nested"] {
                        assert!(!shown.contains(secret), "{input}: {shown}");
                    }
                    assert!(shown.contains("[redacted]"), "{input}: {shown}");
                    assert!(shown.ends_with("next"), "{input}: {shown}");
                }
            }
        }
    }

    #[test]
    fn source_comment_markers_survive_while_embedded_paths_stay_redacted() {
        let source =
            sanitized_command_output("/// doc // line /* block */ render(\"</div>\"); a//b")
                .unwrap();
        assert_eq!(
            source,
            "/// doc // line /* block */ render(\"</div>\"); a//b"
        );
        let embedded =
            sanitized_command_output("open(\"/home/probe/secret.txt\") </home/probe/x>").unwrap();
        assert_eq!(embedded, "[redacted] [redacted]");
    }

    #[test]
    fn input_redirects_and_unicode_paths_are_not_mistaken_for_source_markup() {
        for (input, expected) in [
            ("cat </private.txt", "cat [redacted]"),
            ("cat </private", "cat [redacted]"),
            (
                "echo >/\u{7528}\u{6237}/\u{79c1}\u{5bc6}.txt",
                "echo [redacted]",
            ),
            ("echo >/\u{e9}.txt", "echo [redacted]"),
        ] {
            assert!(sanitize_command_summary(input).is_err(), "{input}");
            assert_eq!(
                sanitized_command_output(input).unwrap(),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn quoted_paths_and_secret_tails_are_fully_redacted_without_hiding_safe_following_text() {
        for input in [
            "--token='private value' safe",
            "--password=\"private value\" safe",
            "KEY='private value' safe",
            "TOKEN=\"private value\" safe",
            "--secret private\\ value safe",
            "--token 'private\nvalue' safe",
            "cat '/home/alice/private file.txt' safe",
            r#"cat "C:\Users\Alice\private file.txt" safe"#,
        ] {
            let output = sanitized_command_output(input).unwrap();
            assert!(!output.contains("private"), "{input}: {output}");
            assert!(!output.contains("value"), "{input}: {output}");
            assert!(!output.contains("file.txt"), "{input}: {output}");
            assert!(output.ends_with(" safe"), "{input}: {output}");
            assert_eq!(
                normalized_text::<512>("toolOutput", input)
                    .unwrap()
                    .as_str(),
                output
            );
        }
        for input in [
            "--token 'private value",
            "PASSWORD=\"private value",
            "Authorization: Basic private value\nnext",
        ] {
            let output = sanitized_command_output(input).unwrap();
            assert!(!output.contains("private"), "{input}: {output}");
            assert!(!output.contains("value"), "{input}: {output}");
        }
    }

    #[test]
    fn redaction_expansion_remains_bounded_for_command_and_output_values() {
        assert!(normalized_text::<2>("toolOutput", "/").is_err());
        assert!(sanitize_command_summary(&format!("{} --token a", "x".repeat(65_526))).is_err());
        let output = sanitized_command_output(&"/ ".repeat(524_288)).unwrap();
        assert_eq!(output.len(), 5_767_168);
        assert!(sanitized_command_output(&"x".repeat(1_048_577)).is_err());
    }

    #[test]
    fn shell_paths_do_not_inherit_source_comment_or_markup_exemptions() {
        for input in [
            "cat /*/private.txt",
            "cat '/*private/notes.txt'",
            "cat </private>public",
            "cat //home/alice/private.txt",
            "cat //server/share/private.txt",
            r"cat \/home/alice/private.txt",
            r"cat C\:\\Users\Alice\private.txt",
        ] {
            assert!(sanitize_command_summary(input).is_err(), "{input}");
            assert!(
                !sanitized_command_output(input).unwrap().contains("private"),
                "{input}"
            );
        }
    }

    #[test]
    fn authorization_headers_and_substitution_values_share_complete_redaction() {
        for input in [
            "echo Authorization: Digest nonce=\"private value\"",
            "AUTH=Bearer 'private value'",
            "curl -H\"Authorization: Basic private value\"",
            "TOKEN=$(printf '%s' 'private value') env",
            "TOKEN=`printf '%s' 'private value'` env",
            "--token $(printf '%s' 'private value') env",
        ] {
            for shown in [
                sanitize_command_summary(input)
                    .unwrap_or_else(|error| panic!("{input}: {error:?}"))
                    .as_str()
                    .to_owned(),
                sanitized_command_output(input).unwrap(),
            ] {
                assert!(!shown.contains("private"), "{input}: {shown}");
                assert!(!shown.contains("value"), "{input}: {shown}");
            }
        }
    }

    #[test]
    fn markdown_wrappers_are_removed_but_unmatched_credential_quotes_fail_closed() {
        for (input, expected) in [
            (
                "Use `token=private` safely afterwards.",
                "Use [redacted] safely afterwards.",
            ),
            ("token=don't ordinary prose", "[redacted]"),
        ] {
            assert_eq!(sanitized_command_output(input).unwrap(), expected);
            assert_eq!(
                normalized_text::<512>("text", input).unwrap().as_str(),
                expected
            );
        }
    }

    #[test]
    fn separators_and_marker_names_inside_secret_values_are_literal_content() {
        for (input, expected) in [
            ("--token \"a=b private\" safe", "--token [redacted] safe"),
            (
                "--token account='first private value' safe",
                "--token [redacted] safe",
            ),
            ("TOKEN=prefix'first private value' safe", "[redacted] safe"),
            ("echo --token=private safe", "echo [redacted] safe"),
            ("echo --token='private' safe", "echo [redacted] safe"),
            (
                "--password 'a:b private' safe",
                "--password [redacted] safe",
            ),
            (
                "--token --password 'private value' safe",
                "--token [redacted] [redacted] safe",
            ),
            ("\"TOKEN=private value\" safe", "[redacted] safe"),
            ("TOKEN='private Bearer' safe", "[redacted] safe"),
            ("TOKEN='private --token' safe", "[redacted] safe"),
            (
                "--secret 'Authorization: private value' safe",
                "--secret [redacted] safe",
            ),
        ] {
            assert_eq!(sanitized_command_output(input).unwrap(), expected);
            assert_eq!(sanitize_command_summary(input).unwrap().as_str(), expected);
        }
    }

    #[test]
    fn redacts_secret_and_native_path_tokens() {
        let value = normalized_text::<512>(
            "fixture",
            r"result sk-ant-secret C:\private\file.txt /private/file Bearer abc123 safe",
        )
        .unwrap();
        assert_eq!(
            value.as_str(),
            "result [redacted] [redacted] [redacted] Bearer [redacted] safe"
        );
    }

    #[test]
    fn an_embedded_absolute_path_behind_a_flag_or_key_is_redacted() {
        let value = normalized_text::<512>(
            "fixture",
            r"Write --target=C:\Users\Alice\secret.txt note:/home/probe/x tail",
        )
        .unwrap();
        assert_eq!(value.as_str(), "Write [redacted] [redacted] tail");
    }

    #[test]
    fn a_web_url_survives_the_embedded_path_redaction() {
        let value =
            normalized_text::<512>("fixture", "Fetch https://example.com/docs and http://a.b/c")
                .unwrap();
        assert_eq!(
            value.as_str(),
            "Fetch https://example.com/docs and http://a.b/c"
        );
    }

    #[test]
    fn control_and_bidi_characters_are_neutralized_before_redaction() {
        let value = normalized_label_text::<512>(
            "fixture",
            "Write\u{7}hr6.txt\u{202e}\u{200b}\u{feff} tail",
        )
        .unwrap();
        assert_eq!(value.as_str(), "Write hr6.txt tail");
    }

    #[test]
    fn tabs_and_newlines_and_nul_survive_neutralization() {
        let value = normalized_text::<512>("fixture", "a\tb\nc").unwrap();
        assert_eq!(value.as_str(), "a\tb\nc");
        assert!(normalized_text::<512>("fixture", "a\u{0}b").is_err());
    }

    #[test]
    fn joiners_survive_message_text_while_labels_still_strip_bidi_overrides() {
        let message = "\u{1f469}\u{200d}\u{1f4bb} \u{62f}\u{200c}\u{646} tail";
        let value = normalized_text::<512>("text", message).unwrap();
        assert_eq!(value.as_str(), message);
        let label = normalized_label_text::<512>("toolLabel", message).unwrap();
        assert_eq!(label.as_str(), "\u{1f469}\u{1f4bb} \u{62f}\u{646} tail");
        let overridden = normalized_text::<512>("text", "a\u{202e}b").unwrap();
        assert_eq!(overridden.as_str(), "a\u{202e}b");
    }

    #[test]
    fn claude_step_08_prose_tool_label_and_read_output_survive_shell_syntax() {
        let prose = "Here's the file's exact contents:\n```text\nread-only harness probe\n```";
        assert_eq!(
            normalized_text::<512>("text", prose).unwrap().as_str(),
            prose
        );
        let label = "Search for `alpha.txt` in the project's files";
        assert_eq!(
            normalized_label_text::<512>("toolLabel", label)
                .unwrap()
                .as_str(),
            label
        );
        let output = "```text\n1\tread-only harness probe\n3\t```";
        assert_eq!(
            normalized_text::<512>("toolOutput", output)
                .unwrap()
                .as_str(),
            output
        );
        assert_eq!(sanitized_command_output(output).unwrap(), output);
        let provider_error = "Internal error: Error from provider (Console): OpenCode's free tier can only be used from within OpenCode";
        assert_eq!(
            normalized_text::<512>("errorMessage", provider_error)
                .unwrap()
                .as_str(),
            provider_error
        );
    }

    #[test]
    fn prose_and_output_redact_only_secret_tokens_and_paths() {
        let input = "Here's token=private Bearer abc123 https://user:pass@example.com/x /home/probe/key.txt safe";
        let result = normalized_text::<512>("text", input).unwrap();
        assert_eq!(
            result.as_str(),
            "Here's [redacted] Bearer [redacted] [redacted] [redacted] safe"
        );
        assert_eq!(sanitized_command_output(input).unwrap(), result.as_str());
    }

    #[test]
    fn authorization_schemes_and_mixed_case_bearer_never_expose_values() {
        for (input, expected) in [
            ("bearer abc123 tail", "bearer [redacted] tail"),
            (
                "\"Bearer\", \t\r\nopaque-command-credential tail",
                "\"Bearer\", \t\r\n[redacted] tail",
            ),
            (
                "Authorization: Basic abc123\nordinary prose",
                "Authorization: [redacted] [redacted]\nordinary prose",
            ),
            (
                "authorization: Digest username=private nonce=secret\nnext",
                "authorization: [redacted] [redacted] [redacted]\nnext",
            ),
            (
                "Proxy-Authorization: Token abc123\nnext",
                "Proxy-Authorization: [redacted] [redacted]\nnext",
            ),
            (
                "Authorization: Bearer\nordinary prose",
                "Authorization: [redacted]\nordinary prose",
            ),
            (
                "Authorization: --token\nordinary prose",
                "Authorization: [redacted]\nordinary prose",
            ),
        ] {
            assert_eq!(
                normalized_text::<512>("text", input).unwrap().as_str(),
                expected
            );
            assert_eq!(sanitized_command_output(input).unwrap(), expected);
        }
    }
}
