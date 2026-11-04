use serde_json::Value;
use super::{BrowserRequest, BrowserScope};

/// Preserve only enough structure to audit a CDP refusal before admission,
/// including malformed arguments or a missing attachment. Storage omits values.
pub(super) fn preflight_request(params: &Value, session: &str, attached_tab: Option<u64>) -> Option<BrowserRequest> {
    if params["name"] != "cdp" { return None; }
    let args = &params["arguments"];
    Some(BrowserRequest {
        scope: BrowserScope {
            session_id: session.to_owned(),
            tab_id: args["scope"]["tabId"].as_u64().or(attached_tab).unwrap_or(0),
        },
        command: super::super::BrowserCommand::Cdp {
            method: args["method"].as_str().unwrap_or("").to_owned(),
            params: args["params"].as_object().cloned().unwrap_or_default(),
        },
    })
}
