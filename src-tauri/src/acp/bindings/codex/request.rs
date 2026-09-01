use serde_json::{Value, json};

use crate::acp::{
    contract::RequiredText,
    intent::{ModelId, Prompt},
    limits::LIMITS,
};

use super::manifest::Platform;
use super::policy::{
    APPROVAL_POLICY_NEVER, SANDBOX_NETWORK_ACCESS, SANDBOX_POLICY_READ_ONLY, SANDBOX_READ_ONLY,
};

// The shared writer owns the single LF delimiter. Keeping the payload one
// byte below the wire ceiling makes the eventual payload + LF write fit the
// existing bounded stdin endpoint without adapter-specific framing.
const JSON_PAYLOAD_BYTES: usize = LIMITS.wire.frame_bytes as usize - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AppServerClientIdentity {
    pub(crate) name: &'static str,
    pub(crate) title: &'static str,
    pub(crate) version: &'static str,
}

pub(crate) const APP_SERVER_CLIENT_IDENTITY: AppServerClientIdentity = AppServerClientIdentity {
    name: "sage",
    title: "Sage",
    version: env!("CARGO_PKG_VERSION"),
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeProjectRoot(String);

impl NativeProjectRoot {
    pub(crate) fn new(platform: Platform, value: String) -> Result<Self, RequestError> {
        if value.is_empty()
            || value.len() > LIMITS.wire.absolute_path_bytes_native_only as usize
            || !is_absolute_normalized(platform, &value)
        {
            return Err(RequestError::InvalidNativePath);
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestKind {
    Initialize,
    Start,
    Resume,
    Read,
    Models,
    TurnStart,
    Interrupt,
    Unsubscribe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutgoingRequest {
    pub(crate) id: u64,
    pub(crate) kind: RequestKind,
    pub(crate) bytes: Vec<u8>,
}

impl OutgoingRequest {
    pub(crate) fn build(id: u64, kind: RequestKind, params: Value) -> Result<Self, RequestError> {
        if id > i64::MAX as u64 {
            return Err(RequestError::InvalidId);
        }
        let method = match kind {
            RequestKind::Initialize => "initialize",
            RequestKind::Start => "thread/start",
            RequestKind::Resume => "thread/resume",
            RequestKind::Read => "thread/read",
            RequestKind::Models => "model/list",
            RequestKind::TurnStart => "turn/start",
            RequestKind::Interrupt => "turn/interrupt",
            RequestKind::Unsubscribe => "thread/unsubscribe",
        };
        let bytes = serde_json::to_vec(&json!({"id": id, "method": method, "params": params}))
            .map_err(|_| RequestError::Encoding)?;
        if bytes.len() > JSON_PAYLOAD_BYTES
            || bytes.iter().any(|byte| matches!(byte, b'\n' | b'\r'))
        {
            return Err(RequestError::FrameBytes);
        }
        Ok(Self { id, kind, bytes })
    }

    pub(crate) fn initialize(id: u64) -> Result<Self, RequestError> {
        let client = APP_SERVER_CLIENT_IDENTITY;
        Self::build(
            id,
            RequestKind::Initialize,
            json!({
                "clientInfo": {
                    "name": client.name,
                    "title": client.title,
                    "version": client.version
                },
                "capabilities": {"experimentalApi": false}
            }),
        )
    }

    pub(crate) fn start(
        id: u64,
        root: &NativeProjectRoot,
        model: Option<&ModelId>,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::Start,
            json!({
                "model": model.map(RequiredText::as_str),
                "cwd": root.as_str(),
                "approvalPolicy": APPROVAL_POLICY_NEVER,
                "sandbox": SANDBOX_READ_ONLY,
                "ephemeral": false
            }),
        )
    }

    pub(crate) fn resume(
        id: u64,
        thread_id: &RequiredText<512>,
        root: &NativeProjectRoot,
        model: Option<&ModelId>,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::Resume,
            json!({
                "threadId": thread_id.as_str(),
                "model": model.map(RequiredText::as_str),
                "cwd": root.as_str(),
                "approvalPolicy": APPROVAL_POLICY_NEVER,
                "sandbox": SANDBOX_READ_ONLY
            }),
        )
    }

    pub(crate) fn read(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::Read,
            json!({"threadId": thread_id.as_str(), "includeTurns": false}),
        )
    }

    pub(crate) fn models(
        id: u64,
        cursor: Option<&RequiredText<512>>,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::Models,
            json!({
                "cursor": cursor.map(RequiredText::as_str),
                "limit": 128,
                "includeHidden": false
            }),
        )
    }

    pub(crate) fn turn_start(
        id: u64,
        thread_id: &RequiredText<512>,
        prompt: &Prompt,
        client_message_id: &RequiredText<512>,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::TurnStart,
            json!({
                "threadId": thread_id.as_str(),
                "clientUserMessageId": client_message_id.as_str(),
                "input": [{"type": "text", "text": prompt.as_str()}],
                "approvalPolicy": APPROVAL_POLICY_NEVER,
                "sandboxPolicy": {
                    "type": SANDBOX_POLICY_READ_ONLY,
                    "networkAccess": SANDBOX_NETWORK_ACCESS
                }
            }),
        )
    }

    pub(crate) fn interrupt(
        id: u64,
        thread_id: &RequiredText<512>,
        turn_id: &RequiredText<512>,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::Interrupt,
            json!({"threadId": thread_id.as_str(), "turnId": turn_id.as_str()}),
        )
    }

    pub(crate) fn unsubscribe(
        id: u64,
        thread_id: &RequiredText<512>,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::Unsubscribe,
            json!({"threadId": thread_id.as_str()}),
        )
    }
}

pub(crate) fn initialized_notification() -> Result<Vec<u8>, RequestError> {
    let bytes = serde_json::to_vec(&json!({"method": "initialized"}))
        .map_err(|_| RequestError::Encoding)?;
    if bytes.len() > JSON_PAYLOAD_BYTES || bytes.iter().any(|byte| matches!(byte, b'\n' | b'\r')) {
        return Err(RequestError::FrameBytes);
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RequestError {
    #[error("request id is outside the admitted integer range")]
    InvalidId,
    #[error("native project path is not absolute and normalized")]
    InvalidNativePath,
    #[error("request serialization failed")]
    Encoding,
    #[error("request exceeds the frame byte limit")]
    FrameBytes,
}

fn is_absolute_normalized(platform: Platform, value: &str) -> bool {
    match platform {
        Platform::WindowsX86_64 => {
            let bytes = value.as_bytes();
            bytes.len() >= 4
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && bytes[2] == b'\\'
                && !value.contains('/')
                && !value.split('\\').enumerate().any(|(index, part)| {
                    part.is_empty()
                        || matches!(part, "." | "..")
                        || (index == 0 && part.len() != 2)
                        || (index > 0
                            && (part.contains(':') || part.ends_with('.') || part.ends_with(' ')))
                })
        }
        Platform::LinuxX86_64 | Platform::MacosAarch64 | Platform::MacosX86_64 => {
            value.starts_with('/')
                && !value
                    .split('/')
                    .skip(1)
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
        }
    }
}
