use serde_json::json;

use super::{IngressError, parse_usage, sanitize};
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
