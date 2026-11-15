use super::*;
use crate::acp::{
    bindings::command_summary::sanitize_command_summary,
    redaction::text::{Mode, redact},
};

#[test]
fn prose_and_labels_preserve_division_markup_and_source_comments() {
    for input in [
        "ratio 3 / 4",
        "close with </div>",
        "```js\n/** docs */\n//TODO keep code\n```",
    ] {
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
    }
}

#[test]
fn prose_secret_values_never_swallow_following_words_or_sentences() {
    for (input, expected) in [
        (
            "the --token isn't stored, so ... Let's continue",
            "the --token [redacted] stored, so ... Let's continue",
        ),
        (
            "Pass `--api-key KEY` to the CLI. Then continue.",
            "Pass `--api-key [redacted] to the CLI. Then continue.",
        ),
        ("token=don't ordinary prose", "[redacted] ordinary prose"),
    ] {
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
}

#[test]
fn separated_assignment_values_are_redacted_in_prose_commands_and_console_output() {
    for input in [
        "password: hunter2 tail",
        "password= hunter2 tail",
        "password : hunter2 tail",
        "password = hunter2 tail",
        "--password = hunter2 tail",
        "--api-key : abc123 tail",
        "\"api_key\": \"abc123\" tail",
        "{\"api_key\"\n: \"abc123\"} tail",
        "password\n= hunter2 tail",
        "curl -H \"X-Api-Key: abc123\" tail",
    ] {
        for shown in [
            normalized_text::<512>("text", input)
                .unwrap()
                .as_str()
                .to_owned(),
            normalized_label_text::<512>("toolLabel", input)
                .unwrap()
                .as_str()
                .to_owned(),
            sanitized_content_output(input).unwrap(),
            sanitized_command_output(input).unwrap(),
            sanitize_command_summary(input).unwrap().as_str().to_owned(),
        ] {
            assert!(!shown.contains("hunter2"), "{input}: {shown}");
            assert!(!shown.contains("abc123"), "{input}: {shown}");
            assert!(!shown.contains("private"), "{input}: {shown}");
            assert!(shown.contains("[redacted]"), "{input}: {shown}");
            assert!(shown.ends_with(" tail"), "{input}: {shown}");
        }
    }
}

#[test]
fn command_and_console_headers_redact_the_complete_quoted_value() {
    let input = "curl -H 'X-Api-Key: private value' tail";
    assert_eq!(
        sanitized_command_output(input).unwrap(),
        "curl -H [redacted] tail"
    );
    assert_eq!(
        sanitize_command_summary(input).unwrap().as_str(),
        "curl -H [redacted] tail"
    );
}

#[test]
fn unmatched_command_quotes_and_backticks_cannot_hide_text_without_a_marker() {
    for input in [
        "echo 'unfinished ordinary text",
        "echo `unfinished ordinary text",
        "echo unfinished' ordinary text",
    ] {
        assert_eq!(
            redact("toolOutput", input, 512, Mode::CommandOutput).unwrap(),
            input
        );
    }
    for input in [
        "--token 'private value",
        "--token `private value",
        "TOKEN=don't private value",
    ] {
        let shown = redact("toolOutput", input, 512, Mode::CommandOutput).unwrap();
        assert!(shown.contains("[redacted]"), "{input}: {shown}");
        assert!(!shown.contains("private"), "{input}: {shown}");
        assert!(!shown.contains("value"), "{input}: {shown}");
    }
    assert_eq!(
        redact(
            "toolOutput",
            "Pass `--api-key KEY` to the CLI. Then continue.",
            512,
            Mode::CommandOutput
        )
        .unwrap(),
        "Pass `--api-key [redacted] to the CLI. Then continue."
    );
}

#[test]
fn at_file_and_attached_short_flag_paths_are_redacted_and_permissions_fail_closed() {
    for input in [
        "curl -d @/home/alice/secret.json",
        "cc -L/home/alice/lib",
        "cc -I/home/alice/inc",
        "cc \"-I/home/alice/inc\"",
        "curl -d @/\u{79d8}\u{5bc6}/file.json",
        "cc -L/\u{79d8}\u{5bc6}/lib",
        "cc -I/\u{79d8}\u{5bc6}/inc",
        r"curl -d @C:\Users\Alice\secret.json",
        r"cc -LC:\Users\Alice\lib",
        r"cc -I\\server\private\inc",
    ] {
        assert!(sanitize_command_summary(input).is_err(), "{input}");
        assert!(
            matches!(
                redact("permissionCommand", input, 512, Mode::Permission),
                Err(
                    sage_acp_protocol::contract::ContractError::ForbiddenContent {
                        field: "permissionCommand"
                    }
                )
            ),
            "{input}"
        );
        for shown in [
            sanitized_command_output(input).unwrap(),
            normalized_text::<512>("text", input)
                .unwrap()
                .as_str()
                .to_owned(),
            normalized_label_text::<512>("toolLabel", input)
                .unwrap()
                .as_str()
                .to_owned(),
            sanitized_content_output(input).unwrap(),
        ] {
            assert!(shown.contains("[redacted]"), "{input}: {shown}");
            for private in ["alice", "Alice", "secret.json", "server", "private"] {
                assert!(!shown.contains(private), "{input}: {shown}");
            }
        }
    }
}

#[test]
fn command_backtick_escaped_secret_whitespace_does_not_leak_the_following_value() {
    for input in ["--token private` value safe", "TOKEN=private` value safe"] {
        let shown = sanitized_command_output(input).unwrap();
        assert!(!shown.contains("private"), "{input}: {shown}");
        assert!(!shown.contains("value"), "{input}: {shown}");
        assert!(shown.contains("[redacted]"), "{input}: {shown}");
    }
}
