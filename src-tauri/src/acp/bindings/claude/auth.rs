use serde_json::Value;

const AUTH_STATUS_UPDATE_METHOD: &str = "_auth/status_update";
const SIGNED_OUT_KINDS: [&str; 5] = ["none", "expired", "loggedOut", "unauthenticated", "error"];
const SIGNED_IN_KINDS: [&str; 1] = ["subscription"];
const MAX_KIND_CHARS: usize = 64;
const AUTH_REQUIRED_CODE: i64 = -32000;
const AUTH_REQUIRED_MESSAGES: [&str; 3] = [
    "authentication required",
    "not logged in",
    "not authenticated",
];
const MAX_ERROR_MESSAGE_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ClaudeAuthState {
    #[default]
    Unobserved,
    SignedOut,
    Unknown,
    SignedIn,
}

impl ClaudeAuthState {
    pub(crate) fn permits_prompt(&self) -> bool {
        !matches!(self, Self::SignedOut)
    }
}

fn bounded(value: Option<&str>, limit: usize) -> Option<&str> {
    let text = value?.trim();
    if text.is_empty() || text.chars().count() > limit {
        return None;
    }
    Some(text)
}

pub(crate) fn decode_auth_status_update(value: &Value) -> Option<ClaudeAuthState> {
    if value.get("method").and_then(Value::as_str)? != AUTH_STATUS_UPDATE_METHOD {
        return None;
    }
    let params = value.get("params")?;
    let params = params.get("authStatus").unwrap_or(params);
    let kind = bounded(params.get("kind").and_then(Value::as_str), MAX_KIND_CHARS)?;
    if SIGNED_OUT_KINDS
        .iter()
        .any(|known_kind| known_kind.eq_ignore_ascii_case(kind))
    {
        return Some(ClaudeAuthState::SignedOut);
    }
    if SIGNED_IN_KINDS
        .iter()
        .any(|known_kind| known_kind.eq_ignore_ascii_case(kind))
    {
        return Some(ClaudeAuthState::SignedIn);
    }
    Some(ClaudeAuthState::Unknown)
}

pub(crate) fn decode_auth_required_error(value: &Value) -> Option<ClaudeAuthState> {
    let error = value.get("error")?;
    if error.get("code").and_then(Value::as_i64)? != AUTH_REQUIRED_CODE {
        return None;
    }
    let message = bounded(
        error.get("message").and_then(Value::as_str),
        MAX_ERROR_MESSAGE_CHARS,
    )?;
    if !AUTH_REQUIRED_MESSAGES
        .iter()
        .any(|known_message| known_message.eq_ignore_ascii_case(message))
    {
        return None;
    }
    Some(ClaudeAuthState::SignedOut)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_observed_signed_out_notification_decodes_instead_of_being_unsupported() {
        let observed = json!({
            "jsonrpc": "2.0",
            "method": "_auth/status_update",
            "params": {"authStatus": {"kind": "none", "label": "Not logged in"}}
        });
        let state = decode_auth_status_update(&observed).expect("decodes");
        assert_eq!(state, ClaudeAuthState::SignedOut);
        assert!(!state.permits_prompt());
    }

    #[test]
    fn an_unfamiliar_credential_kind_is_not_assumed_signed_in() {
        let state = decode_auth_status_update(&json!({
            "method": "_auth/status_update",
            "params": {"authStatus": {"kind": "enterprise-sso", "label": "Acme"}}
        }))
        .expect("decodes");
        assert_eq!(state, ClaudeAuthState::Unknown);
        assert!(state.permits_prompt());
    }

    #[test]
    fn each_observed_signed_out_status_blocks_a_prompt() {
        for kind in SIGNED_OUT_KINDS {
            let state = decode_auth_status_update(&json!({
                "method": "_auth/status_update",
                "params": {"authStatus": {"kind": kind}}
            }))
            .expect("decodes");
            assert!(!state.permits_prompt(), "{kind} must not permit a prompt");
        }
        let state = ClaudeAuthState::default();
        assert!(state.permits_prompt());
    }

    #[test]
    fn a_status_without_a_readable_kind_is_not_decoded() {
        assert!(decode_auth_status_update(&json!({"method": "_auth/status_update"})).is_none());
        assert!(
            decode_auth_status_update(&json!({
                "method": "_auth/status_update",
                "params": {"authStatus": {"kind": ""}}
            }))
            .is_none()
        );
        assert!(
            decode_auth_status_update(&json!({
                "method": "session/update",
                "params": {"authStatus": {"kind": "none"}}
            }))
            .is_none()
        );
    }

    #[test]
    fn a_flat_status_payload_still_decodes_for_adapters_that_do_not_nest_it() {
        let state = decode_auth_status_update(&json!({
            "method": "_auth/status_update",
            "params": {"kind": "none"}
        }))
        .expect("decodes");
        assert_eq!(state, ClaudeAuthState::SignedOut);
    }

    #[test]
    fn the_observed_prompt_refusal_is_read_as_signed_out_rather_than_a_bare_failure() {
        let observed = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "error": {"code": -32000, "message": "Authentication required"}
        });
        let state = decode_auth_required_error(&observed).expect("decodes");
        assert_eq!(state, ClaudeAuthState::SignedOut);
        assert!(!state.permits_prompt());
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
                decode_auth_required_error(&frame).is_none(),
                "unrelated frame decoded as sign-in: {frame}"
            );
        }
    }

    #[test]
    fn the_refusal_message_is_matched_without_regard_to_casing() {
        for message in ["AUTHENTICATION REQUIRED", "Not Logged In"] {
            let state = decode_auth_required_error(&json!({
                "id": 9,
                "error": {"code": -32000, "message": message}
            }))
            .expect("decodes");
            assert!(!state.permits_prompt());
        }
    }

    #[test]
    fn an_mcp_server_authentication_message_is_not_a_harness_auth_refusal() {
        let state = decode_auth_required_error(&json!({
            "id": 9,
            "error": {
                "code": -32000,
                "message": "Authentication required for MCP server GitHub"
            }
        }));
        assert!(state.is_none());
    }
}
