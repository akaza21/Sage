use serde_json::json;

use super::{IngressError, parse_usage, sanitize, sanitized_command_output};
use crate::acp::intent::NormalizedPayload;

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
fn generic_bearer_colon_does_not_carry_redaction_to_the_next_token() {
    assert_eq!(sanitize("Bearer: opaque"), "[redacted] opaque");
}

#[test]
fn a_slash_command_in_prose_is_not_a_path_but_command_output_keeps_it_private() {
    for (prose, shown) in [
        ("re-run /review", "re-run /review"),
        ("(/review).", "(/review)."),
        ("/home/u/x", "[path]"),
        ("`/a/b`", "[path]"),
        ("C:\\x", "[path]"),
        ("file:///x", "[path]"),
        ("/root", "[path]"),
        ("/alice", "[path]"),
        ("x=/review", "[path]"),
    ] {
        assert_eq!(sanitize(prose), shown);
    }
    assert_eq!(sanitized_command_output("/review").unwrap(), "[path]");
}
