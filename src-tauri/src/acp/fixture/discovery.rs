//! What a desktop Codex replay case answers to Sage's model discovery.
//!
//! Real Codex discovery is a short-lived app-server run of its own:
//! `initialize`, `account/read` and `model/list`. A replay case is one scripted
//! session, so its discovery is answered from the case itself: the same
//! `initialize` and `model/list` frames its session steps send, and an
//! account that needs no sign-in. Sage reads them with its real discovery
//! protocol, so the composer's model menu is the catalogue the session sees.
use serde_json::{Value, json};

use crate::acp_protocol::native_cases::reply_shape;

/// The reply to one discovery request, or `None` for a request discovery
/// does not make. Like the Codex app server, a reply carries only its `id`
/// and `result`, which discovery requires.
pub(crate) fn native_case_discovery_reply(case: &str, request: &Value) -> Option<Value> {
    let id = request.get("id")?.as_u64()?;
    let result = match request.get("method")?.as_str()? {
        "account/read" => json!({"account": null, "requiresOpenaiAuth": false}),
        "initialize" => {
            super::wire::codex_reply(&reply_shape(case, "codexInitialize")?, id)?["result"].take()
        }
        "model/list" => {
            super::wire::codex_reply(&reply_shape(case, "codexModelList")?, id)?["result"].take()
        }
        _ => return None,
    };
    result
        .is_object()
        .then(|| json!({"id": id, "result": result}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp_protocol::native_cases::native_case_offers_models;

    #[test]
    fn the_restart_case_offers_both_models_its_session_lists() {
        assert!(native_case_offers_models("codex-config-resume"));
        // The effort menu case's thread runs its own model whatever is asked.
        assert!(!native_case_offers_models("codex-model-menu"));
        assert!(!native_case_offers_models("claude-config-resume"));
        let models = native_case_discovery_reply(
            "codex-config-resume",
            &json!({"id": 3, "method": "model/list", "params": {"cursor": null}}),
        )
        .unwrap();
        assert_eq!(models["id"], 3);
        let ids: Vec<_> = models["result"]["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| (model["id"].clone(), model["hidden"].clone()))
            .collect();
        assert_eq!(
            ids,
            [
                (json!("fixture-model"), json!(false)),
                (json!("fixture-model-deep"), json!(false))
            ]
        );
        let account = native_case_discovery_reply(
            "codex-config-resume",
            &json!({"id": 2, "method": "account/read", "params": {}}),
        )
        .unwrap();
        assert_eq!(
            account["result"],
            json!({"account": null, "requiresOpenaiAuth": false})
        );
        let initialize = native_case_discovery_reply(
            "codex-config-resume",
            &json!({"id": 1, "method": "initialize", "params": {}}),
        )
        .unwrap();
        assert_eq!(initialize["id"], 1);
        assert!(initialize["result"]["userAgent"].is_string());
        assert!(
            native_case_discovery_reply(
                "codex-config-resume",
                &json!({"id": 4, "method": "thread/start"})
            )
            .is_none()
        );
    }
}
