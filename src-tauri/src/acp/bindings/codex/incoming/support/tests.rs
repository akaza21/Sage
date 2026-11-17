use serde_json::json;

use super::{IngressError, parse_usage, sanitize, sanitize_hiding_paths, sanitized_command_output};
use crate::acp::redaction::text::{Mode, redact};
use sage_acp_protocol::intent::NormalizedPayload;

#[test]
fn usage_context_window_is_known_only_for_a_positive_bounded_integer() {
    let known = json!({
        "tokenUsage": {
            "last": { "inputTokens": 3, "outputTokens": 2 },
            "modelContextWindow": 272000
        }
    });
    assert_eq!(
        parse_usage(known.as_object().unwrap()),
        Ok(NormalizedPayload::Usage {
            input_tokens: 3,
            output_tokens: 2,
            context_window: Some(272_000),
        })
    );

    let unknown = json!({
        "tokenUsage": { "last": { "inputTokens": 3, "outputTokens": 2 } }
    });
    assert_eq!(
        parse_usage(unknown.as_object().unwrap()),
        Ok(NormalizedPayload::Usage {
            input_tokens: 3,
            output_tokens: 2,
            context_window: None,
        })
    );
}

#[test]
fn invalid_usage_context_windows_are_unknown_without_rejecting_usage() {
    for invalid in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!(u64::from(u32::MAX) + 1),
        json!("272000"),
        json!(null),
        json!({}),
    ] {
        let params = json!({
            "tokenUsage": {
                "last": { "inputTokens": 3, "outputTokens": 2 },
                "modelContextWindow": invalid
            }
        });
        assert_eq!(
            parse_usage(params.as_object().unwrap()),
            Ok(NormalizedPayload::Usage {
                input_tokens: 3,
                output_tokens: 2,
                context_window: None,
            })
        );
    }
}

#[test]
fn malformed_usage_counts_still_fail_closed() {
    for last in [
        json!({ "outputTokens": 2 }),
        json!({ "inputTokens": 3 }),
        json!({ "inputTokens": -1, "outputTokens": 2 }),
        json!({ "inputTokens": 3, "outputTokens": "2" }),
    ] {
        let params = json!({ "tokenUsage": { "last": last } });
        assert_eq!(
            parse_usage(params.as_object().unwrap()),
            Err(IngressError::Malformed)
        );
    }
}

#[test]
fn a_bearer_colon_hides_the_credential_that_follows_it() {
    assert_eq!(sanitize("Bearer: opaque"), "Bearer: [redacted]");
}

#[test]
fn prose_is_redacted_as_every_harness_does_while_command_output_keeps_paths_private() {
    let home = std::env::home_dir().expect("home folder");
    let home = home.to_str().expect("UTF-8 home folder");
    let separator = if home.contains('\\') { '\\' } else { '/' };
    for prose in [
        "re-run /review",
        "(/review).",
        "/home/u/x",
        "`/a/b`",
        "C:\\x",
        "file:///x",
        "/alice",
        "x=/review",
        "token=abc and Bearer opaque",
        &format!("user {home}{separator}.codex{separator}config.toml is ignored"),
    ] {
        assert_eq!(
            sanitize(prose),
            redact("text", prose, 4096, Mode::Prose).unwrap(),
            "{prose}"
        );
    }
    assert_eq!(sanitize("(/review)."), "(/review).");
    assert_eq!(sanitize("see /home/u/x"), "see /home/u/x");
    assert_eq!(
        sanitize(&format!("user {home}{separator}.config")),
        format!("user ~{separator}.config")
    );
    assert_eq!(sanitized_command_output("/review").unwrap(), "[path]");
    assert_eq!(sanitize_hiding_paths("ls /home/u/x"), "ls [path]");
}
