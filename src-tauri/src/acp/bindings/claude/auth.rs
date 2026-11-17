use serde_json::Value;

const AUTH_STATUS_UPDATE_METHOD: &str = "_auth/status_update";
const MAX_KIND_CHARS: usize = 64;
const AUTH_REQUIRED_CODE: i64 = -32000;
const AUTH_REQUIRED_MESSAGES: [&str; 3] = [
    "authentication required",
    "not logged in",
    "not authenticated",
];
const MAX_ERROR_MESSAGE_CHARS: usize = 256;

fn bounded(value: Option<&str>, limit: usize) -> Option<&str> {
    let text = value?.trim();
    if text.is_empty() || text.chars().count() > limit {
        return None;
    }
    Some(text)
}

/// Claude Code announces its sign-in state, but that state goes stale as
/// soon as the user signs in or out outside the harness, so Sage only
/// recognises the notice and leaves each prompt for Claude Code to answer.
pub(crate) fn is_auth_status_update(value: &Value) -> bool {
    if value.get("method").and_then(Value::as_str) != Some(AUTH_STATUS_UPDATE_METHOD) {
        return false;
    }
    let Some(params) = value.get("params") else {
        return false;
    };
    let params = params.get("authStatus").unwrap_or(params);
    bounded(params.get("kind").and_then(Value::as_str), MAX_KIND_CHARS).is_some()
}

pub(crate) fn is_auth_required_error(value: &Value) -> bool {
    let Some(error) = value.get("error") else {
        return false;
    };
    if error.get("code").and_then(Value::as_i64) != Some(AUTH_REQUIRED_CODE) {
        return false;
    }
    bounded(
        error.get("message").and_then(Value::as_str),
        MAX_ERROR_MESSAGE_CHARS,
    )
    .is_some_and(|message| {
        AUTH_REQUIRED_MESSAGES
            .iter()
            .any(|known_message| known_message.eq_ignore_ascii_case(message))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_observed_status_notification_is_recognised_instead_of_being_unsupported() {
        for params in [
            json!({"authStatus": {"kind": "none", "label": "Not logged in"}}),
            json!({"authStatus": {"kind": "subscription", "label": "Max"}}),
            json!({"kind": "none"}),
        ] {
            assert!(
                is_auth_status_update(&json!({
                    "jsonrpc": "2.0",
                    "method": "_auth/status_update",
                    "params": params
                })),
                "{params}"
            );
        }
    }

    #[test]
    fn a_status_without_a_readable_kind_is_not_recognised() {
        assert!(!is_auth_status_update(
            &json!({"method": "_auth/status_update"})
        ));
        assert!(!is_auth_status_update(&json!({
            "method": "_auth/status_update",
            "params": {"authStatus": {"kind": ""}}
        })));
        assert!(!is_auth_status_update(&json!({
            "method": "session/update",
            "params": {"authStatus": {"kind": "none"}}
        })));
    }

    #[test]
    fn the_observed_prompt_refusal_is_read_as_a_sign_in_refusal_rather_than_a_bare_failure() {
        let observed = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "error": {"code": -32000, "message": "Authentication required"}
        });
        assert!(is_auth_required_error(&observed));
    }

    #[test]
    fn an_unrelated_error_is_never_reported_as_a_sign_in_problem() {
        for frame in [
            json!({"id": 1, "error": {"code": -32000, "message": "Session not found"}}),
            json!({"id": 1, "error": {"code": -32601, "message": "Authentication required"}}),
            json!({"id": 1, "error": {"code": -32000}}),
            json!({"id": 1, "result": {}}),
            json!({"id": 1, "error": {"code": -32000, "message": ""}}),
            json!({"id": 1, "error": {"code": -32000, "message": "x".repeat(MAX_ERROR_MESSAGE_CHARS + 1)}}),
        ] {
            assert!(
                !is_auth_required_error(&frame),
                "unrelated frame decoded as sign-in: {frame}"
            );
        }
    }

    #[test]
    fn the_refusal_message_is_matched_without_regard_to_casing() {
        for message in ["AUTHENTICATION REQUIRED", "Not Logged In"] {
            assert!(is_auth_required_error(&json!({
                "id": 9,
                "error": {"code": -32000, "message": message}
            })));
        }
    }

    #[test]
    fn an_mcp_server_authentication_message_is_not_a_harness_auth_refusal() {
        assert!(!is_auth_required_error(&json!({
            "id": 9,
            "error": {
                "code": -32000,
                "message": "Authentication required for MCP server GitHub"
            }
        })));
    }
}
