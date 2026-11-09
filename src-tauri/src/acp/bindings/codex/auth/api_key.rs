use serde_json::{Value, json};

use super::{CodexAuthError, CodexAuthProtocol, CodexAuthRequest, response_result};
use crate::acp::transport::Frame;

const API_KEY_BYTES: usize = 4_096;

/// An OpenAI API key on its way to Codex, which stores it in its own home.
/// Sage never logs, serializes or retains it.
#[derive(serde::Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct NativeApiKey(String);

impl TryFrom<String> for NativeApiKey {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let value = value.trim();
        if value.is_empty()
            || value.len() > API_KEY_BYTES
            || value.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err("invalid API key");
        }
        Ok(Self(value.to_owned()))
    }
}

impl std::fmt::Debug for NativeApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeApiKey([redacted])")
    }
}

impl CodexAuthProtocol {
    pub(crate) fn api_key_login_request(
        &mut self,
        key: &NativeApiKey,
    ) -> Result<CodexAuthRequest, CodexAuthError> {
        self.request(
            "account/login/start",
            Some(json!({"type":"apiKey","apiKey":key.0})),
        )
    }

    pub(crate) fn accept_api_key_login(
        &self,
        frame: &Frame,
        expected_id: u64,
    ) -> Result<(), CodexAuthError> {
        let result = response_result(frame, expected_id)?;
        if result.len() == 1 && result.get("type").and_then(Value::as_str) == Some("apiKey") {
            Ok(())
        } else {
            Err(CodexAuthError::Protocol)
        }
    }
}
