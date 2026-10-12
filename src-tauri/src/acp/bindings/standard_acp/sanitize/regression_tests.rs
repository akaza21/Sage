use super::*;
use crate::acp::bindings::command_summary::{redaction::redact, sanitize_command_summary};

fn assert_preserved(input: &str) {
    assert_eq!(
        normalized_text::<512>("text", input).unwrap().as_str(),
        input
    );
    assert_eq!(
        normalized_label_text::<512>("toolLabel", input)
            .unwrap()
            .as_str(),
        input
    );
    assert_eq!(sanitized_content_output(input).unwrap(), input);
    assert_eq!(sanitized_command_output(input).unwrap(), input);
}

#[test]
fn author_is_not_a_credential_key() {
    assert_preserved("Author: Jane");
}

#[test]
fn token_count_is_not_a_credential_key() {
    assert_preserved("Tokens: 1,234");
}

#[test]
fn session_number_is_not_a_credential_key() {
    assert_preserved("Session: 3");
}

#[test]
fn keyboard_is_not_a_credential_key() {
    assert_preserved("Keyboard: US");
}

#[test]
fn authentication_status_is_not_a_credential_key() {
    assert_preserved("Authentication: done");
}

#[test]
fn git_log_author_is_preserved_in_console_output() {
    let input = "Author: Jane Doe <j@x>";
    assert_eq!(sanitized_command_output(input).unwrap(), input);
}

#[test]
fn author_apostrophe_cannot_swallow_following_output_lines() {
    assert_preserved("Author: O'Brien\nordinary output\nlast line");
}

#[test]
fn an_open_console_secret_value_keeps_following_lines_private() {
    assert_eq!(
        sanitized_command_output("TOKEN='private words\nordinary output\nlast line").unwrap(),
        "[redacted]"
    );
}

#[test]
fn console_secret_quotes_continue_until_their_closing_quote() {
    assert_eq!(
        sanitized_command_output("--token 'private\nvalue' safe").unwrap(),
        "--token [redacted] safe"
    );
}

fn assert_prose(input: &str, expected: &str) {
    assert_eq!(
        normalized_text::<512>("text", input).unwrap().as_str(),
        expected
    );
    assert_eq!(
        normalized_label_text::<512>("toolLabel", input)
            .unwrap()
            .as_str(),
        expected
    );
    assert_eq!(sanitized_content_output(input).unwrap(), expected);
}

#[test]
fn quoted_environment_password_redacts_all_words_without_dropping_prose() {
    assert_prose(
        "before DB_PASSWORD=\"two words\" after",
        "before [redacted] after",
    );
}

#[test]
fn quoted_spaced_password_redacts_all_words_without_dropping_prose() {
    assert_prose(
        "before password = \"a b\" after",
        "before password = [redacted] after",
    );
}

#[test]
fn quoted_json_authorization_redacts_the_scheme_and_secret_without_dropping_prose() {
    assert_prose(
        "before {\"Authorization\": \"Basic abc\"} after",
        "before {\"Authorization\": [redacted]} after",
    );
}

#[test]
fn unclosed_prose_secret_quotes_keep_following_lines_private() {
    assert_prose(
        "before password = \"a b\nafter ordinary prose",
        "before password = [redacted]",
    );
}

#[test]
fn a_quoted_secret_keeps_adjacent_prose_after_its_closing_quote() {
    assert_prose("before password=\"a b\",after", "before [redacted],after");
}

#[test]
fn a_quoted_secret_keeps_escaped_quotes_and_their_tails_private() {
    assert_prose(
        r#"before password="a \"b c" after"#,
        "before [redacted] after",
    );
}

#[test]
fn escaped_secret_whitespace_does_not_escape_the_closing_quote() {
    assert_prose(
        r#"before password="two\ " after"#,
        "before [redacted] after",
    );
}

#[test]
fn a_closing_secret_quote_does_not_expose_a_following_assignment() {
    assert_prose(r#"password="a b";token=abc"#, "[redacted];[redacted]");
}

#[test]
fn a_closing_secret_quote_does_not_expose_a_following_path() {
    assert_prose(
        r#"password="a b",/home/alice/private"#,
        "[redacted],[redacted]",
    );
}

#[test]
fn leading_quoted_secret_whitespace_does_not_expose_the_value() {
    assert_prose(r#"DB_PASSWORD=" two words" after"#, "[redacted] after");
}

#[test]
fn a_pending_quoted_prose_secret_keeps_following_lines_private() {
    assert_prose("DB_PASSWORD=\"\nordinary prose", "[redacted]");
}

#[test]
fn a_pending_quoted_console_secret_keeps_following_lines_private() {
    assert_eq!(
        sanitized_command_output("DB_PASSWORD=\"\nordinary output").unwrap(),
        "[redacted]"
    );
}

#[test]
fn quoted_authorization_does_not_hide_following_prose() {
    assert_prose(
        r#"before Authorization: "Basic abc" after"#,
        "before Authorization: [redacted] after",
    );
}

#[test]
fn an_unquoted_windows_path_redacts_the_segment_after_a_space() {
    let input = r"before C:\Users\John Smith\.ssh\id after";
    let expected = "before [redacted] [redacted] after";
    assert_prose(input, expected);
    assert_eq!(sanitized_command_output(input).unwrap(), expected);
    assert!(sanitize_command_summary(input).is_err());
    assert!(redact("permissionCommand", input, 512, true).is_err());
}

fn assert_glued_path_rejected(input: &str) {
    assert_prose(input, "cc [redacted]");
    assert_eq!(sanitized_command_output(input).unwrap(), "cc [redacted]");
    assert!(sanitize_command_summary(input).is_err(), "{input}");
    assert!(matches!(
        redact("permissionCommand", input, 512, true),
        Err(crate::acp::contract::ContractError::ForbiddenContent {
            field: "permissionCommand"
        })
    ));
}

#[test]
fn a_glued_isystem_path_is_redacted_and_permission_commands_fail_closed() {
    assert_glued_path_rejected("cc -isystem/home/alice/inc");
}

#[test]
fn a_glued_iquote_path_is_redacted_and_permission_commands_fail_closed() {
    assert_glued_path_rejected("cc -iquote/home/alice/x");
}

#[test]
fn an_authorization_substring_is_not_an_authorization_header() {
    assert_preserved("preauthorization: done ordinary prose");
}

#[test]
fn a_secret_camel_case_segment_remains_redacted() {
    let input = "clientSecret=opaque tail";
    assert_prose(input, "[redacted] tail");
    assert_eq!(sanitized_command_output(input).unwrap(), "[redacted] tail");
    assert_eq!(
        sanitize_command_summary(input).unwrap().as_str(),
        "[redacted] tail"
    );
}

#[test]
fn an_api_key_camel_case_segment_remains_redacted() {
    let input = "apiKey=opaque tail";
    assert_prose(input, "[redacted] tail");
    assert_eq!(
        sanitize_command_summary(input).unwrap().as_str(),
        "[redacted] tail"
    );
}

#[test]
fn a_secret_dot_segment_remains_redacted() {
    let input = "service.access.token=opaque tail";
    assert_prose(input, "[redacted] tail");
    assert_eq!(
        sanitize_command_summary(input).unwrap().as_str(),
        "[redacted] tail"
    );
}

#[test]
fn a_private_key_dash_segment_remains_redacted() {
    let input = "private-key=opaque tail";
    assert_prose(input, "[redacted] tail");
    assert_eq!(
        sanitize_command_summary(input).unwrap().as_str(),
        "[redacted] tail"
    );
}

#[test]
fn windows_path_continuation_does_not_redact_the_next_line() {
    let input = "C:\\Users\\John\nordinary\\text";
    assert_prose(input, "[redacted]\nordinary\\text");
    assert_eq!(
        sanitized_command_output(input).unwrap(),
        "[redacted]\nordinary\\text"
    );
}

#[test]
fn long_chained_quoted_values_keep_their_suffixes_bounded_without_recursion() {
    let count = 32_768;
    let input = format!("{} tail", "password=\"a\";".repeat(count));
    let expected = format!("{} tail", "[redacted];".repeat(count));
    assert_eq!(sanitized_content_output(&input).unwrap(), expected);
}
