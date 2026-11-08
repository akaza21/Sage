use serde_json::{Value, json};

use super::profile::StandardAcpProfile;
use crate::acp::mcp::{McpAdmission, McpCapabilities};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    New,
    Initializing,
    Ready,
    OpeningSession,
    Active,
}

pub(crate) struct StandardAcpHandshake {
    profile: &'static StandardAcpProfile,
    stage: Stage,
    mcp: McpAdmission,
    mcp_capabilities: McpCapabilities,
    session_request_id: u64,
}

impl StandardAcpHandshake {
    pub(crate) fn new(profile: &'static StandardAcpProfile) -> Self {
        Self {
            profile,
            stage: Stage::New,
            mcp: McpAdmission::default(),
            mcp_capabilities: McpCapabilities::default(),
            session_request_id: 2,
        }
    }

    pub(crate) fn with_mcp(profile: &'static StandardAcpProfile, mcp: McpAdmission) -> Self {
        Self {
            mcp,
            ..Self::new(profile)
        }
    }

    pub(crate) fn initialize(&mut self) -> Result<Value, StandardAcpHandshakeError> {
        if self.stage != Stage::New {
            return Err(StandardAcpHandshakeError::Operation);
        }
        self.stage = Stage::Initializing;
        let mut capabilities = json!({
            "fs": {
                "readTextFile": self.profile.client_capabilities.read_text_file,
                "writeTextFile": self.profile.client_capabilities.write_text_file,
            }
        });
        if self.profile.elicitation {
            capabilities["terminal"] = Value::Bool(false);
            capabilities["elicitation"] = super::elicitation::client_capability();
        }
        Ok(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": 1,
                "clientCapabilities": capabilities,
                "clientInfo": { "name": "Sage", "version": env!("CARGO_PKG_VERSION") }
            }
        }))
    }

    pub(crate) fn accept_initialize(
        &mut self,
        response: &Value,
    ) -> Result<(), StandardAcpHandshakeError> {
        if self.stage != Stage::Initializing
            || response.get("id").and_then(Value::as_u64) != Some(1)
            || response
                .get("result")
                .and_then(|result| result.get("protocolVersion"))
                .and_then(Value::as_u64)
                != Some(1)
        {
            return Err(StandardAcpHandshakeError::Frame);
        }
        self.stage = Stage::Ready;
        self.mcp_capabilities = McpCapabilities::from_initialize(response);
        Ok(())
    }

    pub(crate) fn new_session(&mut self, cwd: &str) -> Result<Value, StandardAcpHandshakeError> {
        if self.stage != Stage::Ready || cwd.is_empty() {
            return Err(StandardAcpHandshakeError::Operation);
        }
        self.stage = Stage::OpeningSession;
        Ok(json!({
            "jsonrpc": "2.0",
            "id": self.session_request_id,
            "method": "session/new",
            "params": { "cwd": cwd, "mcpServers": self.mcp.wire(self.mcp_capabilities) }
        }))
    }

    pub(crate) fn accept_new_session(
        &mut self,
        response: &Value,
    ) -> Result<String, StandardAcpHandshakeError> {
        if self.stage != Stage::OpeningSession
            || response.get("id").and_then(Value::as_u64) != Some(self.session_request_id)
        {
            return Err(StandardAcpHandshakeError::Frame);
        }
        if auth_refusal(response) {
            self.stage = Stage::Ready;
            return Err(StandardAcpHandshakeError::SignInRequired);
        }
        let session_id = response
            .get("result")
            .and_then(|result| result.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 512)
            .ok_or(StandardAcpHandshakeError::Frame)?
            .to_owned();
        self.stage = Stage::Active;
        Ok(session_id)
    }

    /// After the agent refused `session/new` for sign-in and the client
    /// authenticated, the next `session/new` carries request id `id`.
    pub(crate) fn reopen_session_as(&mut self, id: u64) -> Result<(), StandardAcpHandshakeError> {
        if self.stage != Stage::Ready || id <= self.session_request_id {
            return Err(StandardAcpHandshakeError::Operation);
        }
        self.session_request_id = id;
        Ok(())
    }
}

fn auth_refusal(response: &Value) -> bool {
    response
        .get("error")
        .filter(|error| error.get("code").and_then(Value::as_i64) == Some(-32000))
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .is_some_and(|message| {
            message.len() <= 512
                && message.split_ascii_whitespace().any(|word| {
                    matches!(
                        word.to_ascii_lowercase().as_str(),
                        "auth" | "authentication" | "login" | "signin" | "sign-in"
                    )
                })
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum StandardAcpHandshakeError {
    #[error("standard ACP frame is invalid")]
    Frame,
    #[error("standard ACP operation is unavailable in the current state")]
    Operation,
    #[error("standard ACP harness requires sign-in")]
    SignInRequired,
}
