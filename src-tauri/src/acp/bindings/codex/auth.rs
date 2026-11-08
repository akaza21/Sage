use serde_json::{Map, Value, json};

use crate::acp::{
    bindings::codex::{
        discovery::schema::{api_key_account, managed_chatgpt},
        incoming::{IncomingEvent, IngressDisposition},
        manifest::MetadataValidatedPacket,
        state::CodexAdapter,
    },
    contract::{RuntimeEpoch, SageAgentSessionId},
    limits::LIMITS,
    transport::{Frame, OutboundFrame},
};

const AUTH_URL_BYTES: usize = 16_384;
const LOGIN_ID_BYTES: usize = 64;
const AUTH_ERROR_BYTES: usize = 8_192;

mod api_key;
mod browser;
mod notification;
mod router;
mod runner;
mod transport;
pub(crate) use api_key::NativeApiKey;
pub(crate) use browser::NativeBrowser;
pub(crate) use notification::{CodexAuthMode, CodexAuthNotification};
pub(crate) use runner::{
    AuthBrowser, AuthTransport, CodexAuthDeadline, CodexAuthOutcome, run_account_protocol,
    run_api_key_protocol, run_login_protocol, run_logout_protocol,
};
pub(crate) use transport::ProductionAuthTransport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CodexAuthError {
    #[error("Codex authentication protocol failed")]
    Protocol,
    #[error("Codex authentication URL is outside policy")]
    UrlPolicy,
}

/// Native-only login correlation. This value must never cross IPC or persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeLoginId(String);

impl NativeLoginId {
    pub(super) fn parse(value: &str) -> Result<Self, CodexAuthError> {
        if value.is_empty()
            || value.len() > LOGIN_ID_BYTES
            || value.contains('\0')
            || uuid::Uuid::parse_str(value).is_err()
        {
            return Err(CodexAuthError::Protocol);
        }
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Native-only authorization URL. Its opaque query is never logged or retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeAuthUrl(String);

impl NativeAuthUrl {
    pub(super) fn parse(value: &str) -> Result<Self, CodexAuthError> {
        if value.is_empty() || value.len() > AUTH_URL_BYTES || value.contains(['\0', '\r', '\n']) {
            return Err(CodexAuthError::UrlPolicy);
        }
        let url = url::Url::parse(value).map_err(|_| CodexAuthError::UrlPolicy)?;
        if url.scheme() != "https"
            || url.host_str() != Some("auth.openai.com")
            || !matches!(url.port(), None | Some(443))
            || url.path() != "/oauth/authorize"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_none()
        {
            return Err(CodexAuthError::UrlPolicy);
        }
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) struct CodexAuthRequest {
    id: u64,
    frame: OutboundFrame,
}

impl CodexAuthRequest {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn payload(&self) -> &[u8] {
        self.frame.payload()
    }

    pub(crate) fn into_frame(self) -> OutboundFrame {
        self.frame
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexAuthAccount {
    SignedOut,
    SignedIn,
}

pub(crate) struct CodexAuthProtocol {
    adapter: CodexAdapter,
    next_request_id: u64,
}

impl CodexAuthProtocol {
    pub(crate) fn new(
        packet: &MetadataValidatedPacket,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
    ) -> Self {
        Self::with_adapter(CodexAdapter::from_metadata_packet(
            packet,
            binding_digest,
            runtime_epoch,
            SageAgentSessionId::parse(format!("session-codex-auth-{}", runtime_epoch.get()))
                .expect("bounded auth identity"),
        ))
    }

    pub(crate) fn from_admitted_binding(
        binding: &crate::acp::bindings::codex::AdmittedCodexBinding,
        runtime_epoch: RuntimeEpoch,
    ) -> Self {
        Self::with_adapter(CodexAdapter::from_admitted_binding(
            binding,
            runtime_epoch,
            SageAgentSessionId::parse(format!("session-codex-auth-{}", runtime_epoch.get()))
                .expect("bounded auth identity"),
        ))
    }

    fn with_adapter(adapter: CodexAdapter) -> Self {
        Self {
            adapter,
            next_request_id: 0,
        }
    }

    pub(crate) fn initialize_request(&mut self) -> Result<CodexAuthRequest, CodexAuthError> {
        let id = self.next_id()?;
        let request = self
            .adapter
            .initialize(id)
            .map_err(|_| CodexAuthError::Protocol)?;
        let frame = OutboundFrame::new(request.bytes).map_err(|_| CodexAuthError::Protocol)?;
        Ok(CodexAuthRequest { id, frame })
    }

    pub(crate) fn accept_initialize(
        &mut self,
        frame: &Frame,
        expected_id: u64,
    ) -> Result<OutboundFrame, CodexAuthError> {
        require_response_id(frame, expected_id)?;
        let disposition = self
            .adapter
            .ingest(frame.as_bytes(), &NoResources)
            .map_err(|_| CodexAuthError::Protocol)?;
        let IngressDisposition::Events(events) = disposition else {
            return Err(CodexAuthError::Protocol);
        };
        if !matches!(events.as_slice(), [IncomingEvent::Initialized { .. }]) {
            return Err(CodexAuthError::Protocol);
        }
        let bytes = self
            .adapter
            .initialized()
            .map_err(|_| CodexAuthError::Protocol)?;
        OutboundFrame::new(bytes).map_err(|_| CodexAuthError::Protocol)
    }

    pub(crate) fn account_request(&mut self) -> Result<CodexAuthRequest, CodexAuthError> {
        self.request("account/read", Some(json!({})))
    }

    pub(crate) fn accept_account(
        &self,
        frame: &Frame,
        expected_id: u64,
    ) -> Result<CodexAuthAccount, CodexAuthError> {
        let result = response_result(frame, expected_id)?;
        if result.len() != 2
            || !result.contains_key("account")
            || result
                .get("requiresOpenaiAuth")
                .and_then(Value::as_bool)
                .is_none()
        {
            return Err(CodexAuthError::Protocol);
        }
        match result.get("account") {
            Some(Value::Null) => Ok(CodexAuthAccount::SignedOut),
            Some(Value::Object(account))
                if api_key_account(account)
                    || managed_chatgpt(account).map_err(|_| CodexAuthError::Protocol)? =>
            {
                Ok(CodexAuthAccount::SignedIn)
            }
            _ => Err(CodexAuthError::Protocol),
        }
    }

    pub(crate) fn login_request(&mut self) -> Result<CodexAuthRequest, CodexAuthError> {
        self.request(
            "account/login/start",
            Some(json!({"type":"chatgpt","appBrand":null})),
        )
    }

    pub(crate) fn cancel_request(
        &mut self,
        login_id: &NativeLoginId,
    ) -> Result<CodexAuthRequest, CodexAuthError> {
        self.request(
            "account/login/cancel",
            Some(json!({"loginId":login_id.as_str()})),
        )
    }

    pub(crate) fn logout_request(&mut self) -> Result<CodexAuthRequest, CodexAuthError> {
        self.request("account/logout", None)
    }

    pub(crate) fn accept_login_started(
        &self,
        frame: &Frame,
        expected_id: u64,
    ) -> Result<(NativeLoginId, NativeAuthUrl), CodexAuthError> {
        let result = response_result(frame, expected_id)?;
        if result.len() != 3 || result.get("type").and_then(Value::as_str) != Some("chatgpt") {
            return Err(CodexAuthError::Protocol);
        }
        let login_id = result
            .get("loginId")
            .and_then(Value::as_str)
            .ok_or(CodexAuthError::Protocol)
            .and_then(NativeLoginId::parse)?;
        let auth_url = result
            .get("authUrl")
            .and_then(Value::as_str)
            .ok_or(CodexAuthError::Protocol)
            .and_then(NativeAuthUrl::parse)?;
        Ok((login_id, auth_url))
    }

    pub(crate) fn accept_empty_response(
        &self,
        frame: &Frame,
        expected_id: u64,
    ) -> Result<(), CodexAuthError> {
        if response_result(frame, expected_id)?.is_empty() {
            Ok(())
        } else {
            Err(CodexAuthError::Protocol)
        }
    }

    pub(crate) fn accept_cancel_response(
        &self,
        frame: &Frame,
        expected_id: u64,
    ) -> Result<bool, CodexAuthError> {
        let result = response_result(frame, expected_id)?;
        if result.len() != 1 {
            return Err(CodexAuthError::Protocol);
        }
        match result.get("status").and_then(Value::as_str) {
            Some("canceled") => Ok(true),
            Some("notFound") => Ok(false),
            _ => Err(CodexAuthError::Protocol),
        }
    }

    fn request(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<CodexAuthRequest, CodexAuthError> {
        let id = self.next_id()?;
        let value = match params {
            Some(params) => json!({"id":id,"method":method,"params":params}),
            None => json!({"id":id,"method":method}),
        };
        let bytes = serde_json::to_vec(&value).map_err(|_| CodexAuthError::Protocol)?;
        if bytes.len() >= LIMITS.wire.frame_bytes as usize
            || bytes.iter().any(|byte| matches!(byte, b'\n' | b'\r'))
        {
            return Err(CodexAuthError::Protocol);
        }
        let frame = OutboundFrame::new(bytes).map_err(|_| CodexAuthError::Protocol)?;
        Ok(CodexAuthRequest { id, frame })
    }

    fn next_id(&mut self) -> Result<u64, CodexAuthError> {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .filter(|id| *id <= i64::MAX as u64)
            .ok_or(CodexAuthError::Protocol)?;
        Ok(self.next_request_id)
    }
}

fn response_result(frame: &Frame, expected: u64) -> Result<Map<String, Value>, CodexAuthError> {
    let value: Value =
        serde_json::from_slice(frame.as_bytes()).map_err(|_| CodexAuthError::Protocol)?;
    crate::acp::bindings::codex::incoming::validate_shape(&value)
        .map_err(|_| CodexAuthError::Protocol)?;
    let object = value.as_object().ok_or(CodexAuthError::Protocol)?;
    if object.len() != 2
        || object.get("id").and_then(Value::as_u64) != Some(expected)
        || object.contains_key("error")
        || object.contains_key("method")
    {
        return Err(CodexAuthError::Protocol);
    }
    object
        .get("result")
        .and_then(Value::as_object)
        .cloned()
        .ok_or(CodexAuthError::Protocol)
}

fn require_response_id(frame: &Frame, expected: u64) -> Result<(), CodexAuthError> {
    response_result(frame, expected).map(drop)
}

struct NoResources;

impl crate::acp::bindings::codex::incoming::ResourceMapper for NoResources {
    fn map_existing(
        &self,
        _: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>
    {
        Err(crate::acp::intent::UnavailableResourceReason::Unsupported)
    }
}

#[cfg(test)]
mod tests;
