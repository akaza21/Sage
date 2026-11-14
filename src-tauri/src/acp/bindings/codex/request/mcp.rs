use serde_json::{Value, json};

use crate::acp::contract::RequiredText;

use super::{OutgoingRequest, RequestError, RequestKind};

/// One page holds every server `/mcp` lists.
const MCP_PAGE: u32 = crate::acp::mcp::inventory::MAX_LISTED_SERVERS as u32;

impl OutgoingRequest {
    pub(crate) fn mcp_list(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::McpList,
            json!({
                "threadId": thread_id.as_str(),
                "detail": "toolsAndAuthOnly",
                "limit": MCP_PAGE
            }),
        )
    }

    pub(crate) fn mcp_reload(id: u64) -> Result<Self, RequestError> {
        Self::build(id, RequestKind::McpReload, Value::Null)
    }

    pub(crate) fn mcp_sign_in(
        id: u64,
        thread_id: &RequiredText<512>,
        name: &str,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::McpSignIn,
            json!({"name": name, "threadId": thread_id.as_str()}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(request: OutgoingRequest) -> (String, Value) {
        let value: Value = serde_json::from_slice(&request.bytes).unwrap();
        assert!(!request.kind.starts_turn());
        (
            value["method"].as_str().unwrap().to_owned(),
            value["params"].clone(),
        )
    }

    #[test]
    fn mcp_requests_match_the_pinned_schema() {
        let thread = RequiredText::new("threadId", "thread-1").unwrap();
        assert_eq!(
            wire(OutgoingRequest::mcp_list(1, &thread).unwrap()),
            (
                "mcpServerStatus/list".to_owned(),
                json!({"threadId": "thread-1", "detail": "toolsAndAuthOnly", "limit": 64})
            )
        );
        assert_eq!(
            wire(OutgoingRequest::mcp_reload(2).unwrap()),
            ("config/mcpServer/reload".to_owned(), Value::Null)
        );
        assert_eq!(
            wire(OutgoingRequest::mcp_sign_in(3, &thread, "notion").unwrap()),
            (
                "mcpServer/oauth/login".to_owned(),
                json!({"name": "notion", "threadId": "thread-1"})
            )
        );
        let pinned: Value = serde_json::from_str(include_str!(
            "../../../../../testdata/acp/codex/observed-effort-schema.json"
        ))
        .unwrap();
        let methods = pinned["clientRequestMethods"].as_array().unwrap();
        for method in [
            "mcpServerStatus/list",
            "config/mcpServer/reload",
            "mcpServer/oauth/login",
        ] {
            assert!(methods.iter().any(|known| known == method), "{method}");
        }
    }
}
