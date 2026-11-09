use serde_json::{Value, json};

use crate::acp::{
    contract::RequiredText,
    intent::{ModelId, Prompt},
    limits::LIMITS,
    runtime::host_browse::MAX_HARNESS_BROWSE_URL_BYTES,
};

use super::CODEX_BROWSE_TOOL_NAME;
use super::manifest::Platform;
use super::policy::{APPROVAL_POLICY_NEVER, SANDBOX_READ_ONLY};
use super::turn_policy::{TurnPolicySurface, TurnSettings, turn_policy, validate_turn_policy};

mod session_action;

// The shared writer owns the single LF delimiter. Keeping the payload one
// byte below the wire ceiling makes the eventual payload + LF write fit the
// existing bounded stdin endpoint without adapter-specific framing.
const JSON_PAYLOAD_BYTES: usize = LIMITS.wire.frame_bytes as usize - 1;

/// High-volume notifications Sage never projects, suppressed by Codex at the
/// source: the whole-turn unified diff (Sage reads each file change from its
/// own item) and the deprecated apply_patch output stream.
const OPTED_OUT_NOTIFICATIONS: [&str; 2] = ["turn/diff/updated", "item/fileChange/outputDelta"];

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
    ChildRead,
    Fork,
    /// `thread/unsubscribe` of a thread this runtime forked, never its own.
    ForkRelease,
    /// `thread/delete` of a refused fork: unloads it and removes its rollout.
    ForkDelete,
    Models,
    TurnStart,
    /// `review/start`: an inline review that runs as the correlated Turn.
    Review,
    /// `thread/compact/start`: a compaction that runs as the correlated Turn.
    Compact,
    Steer,
    Interrupt,
    Unsubscribe,
    GoalGet,
    GoalPause,
    GoalClear,
    RateLimitsRead,
}

impl RequestKind {
    pub(crate) fn starts_turn(self) -> bool {
        matches!(self, Self::TurnStart | Self::Review | Self::Compact)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutgoingRequest {
    pub(crate) id: u64,
    pub(crate) kind: RequestKind,
    pub(crate) bytes: Vec<u8>,
}

impl OutgoingRequest {
    pub(crate) fn with_config(self, config: Value) -> Result<Self, RequestError> {
        let mut value: Value =
            serde_json::from_slice(&self.bytes).map_err(|_| RequestError::Encoding)?;
        value["params"]["config"] = config;
        Self::build(self.id, self.kind, value["params"].clone())
    }

    pub(crate) fn build(id: u64, kind: RequestKind, params: Value) -> Result<Self, RequestError> {
        if id > i64::MAX as u64 {
            return Err(RequestError::InvalidId);
        }
        let method = match kind {
            RequestKind::Initialize => "initialize",
            RequestKind::Start => "thread/start",
            RequestKind::Resume => "thread/resume",
            RequestKind::Read => "thread/read",
            RequestKind::ChildRead => "thread/read",
            RequestKind::Fork => "thread/fork",
            RequestKind::Models => "model/list",
            RequestKind::TurnStart => "turn/start",
            RequestKind::Review => "review/start",
            RequestKind::Compact => "thread/compact/start",
            RequestKind::Steer => "turn/steer",
            RequestKind::Interrupt => "turn/interrupt",
            RequestKind::Unsubscribe | RequestKind::ForkRelease => "thread/unsubscribe",
            RequestKind::ForkDelete => "thread/delete",
            RequestKind::GoalGet => "thread/goal/get",
            RequestKind::GoalPause => "thread/goal/set",
            RequestKind::GoalClear => "thread/goal/clear",
            RequestKind::RateLimitsRead => "account/rateLimits/read",
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
                // Codex requires this initialize capability before it
                // accepts the documented thread/start.dynamicTools field.
                "capabilities": {
                    "experimentalApi": true,
                    "optOutNotificationMethods": OPTED_OUT_NOTIFICATIONS
                }
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
                "ephemeral": false,
                "dynamicTools": codex_dynamic_tools()
            }),
        )
    }

    /// `thread/resume` without the thread's Turns, which Sage does not
    /// import; asking for them draws a `deprecationNotice`.
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
                "sandbox": SANDBOX_READ_ONLY,
                "excludeTurns": true
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

    pub(crate) fn child_read(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::ChildRead,
            json!({"threadId": thread_id.as_str(), "includeTurns": true}),
        )
    }

    /// `thread/fork` through the latest Turn. The pinned schema
    /// requires only `threadId`; omitted overrides inherit the source thread,
    /// and `lastTurnId` is left out because Sage forks only between Turns.
    /// Sage imports no native history, so the forked Turns are excluded;
    /// asking for them draws a `deprecationNotice` from the pinned server.
    pub(crate) fn fork(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::Fork,
            json!({"threadId": thread_id.as_str(), "excludeTurns": true}),
        )
    }

    pub(crate) fn goal_get(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::GoalGet,
            json!({"threadId": thread_id.as_str()}),
        )
    }

    pub(crate) fn goal_pause(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::GoalPause,
            json!({"threadId": thread_id.as_str(), "status": "paused"}),
        )
    }

    pub(crate) fn goal_clear(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::GoalClear,
            json!({"threadId": thread_id.as_str()}),
        )
    }

    pub(crate) fn rate_limits_read(id: u64) -> Result<Self, RequestError> {
        Self::build(id, RequestKind::RateLimitsRead, Value::Null)
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
        Self::turn_start_with_access_mode(
            id,
            thread_id,
            prompt,
            client_message_id,
            crate::settings_store::SessionAccessMode::ReadOnly,
            TurnPolicySurface::Unsupported,
            TurnSettings::default(),
        )
    }

    pub(crate) fn turn_start_with_access_mode(
        id: u64,
        thread_id: &RequiredText<512>,
        prompt: &Prompt,
        client_message_id: &RequiredText<512>,
        access_mode: crate::settings_store::SessionAccessMode,
        surface: TurnPolicySurface,
        settings: TurnSettings<'_>,
    ) -> Result<Self, RequestError> {
        let policy = turn_policy(surface, access_mode)?;
        validate_turn_policy(surface, access_mode, policy)?;
        let mut params = json!({
            "threadId": thread_id.as_str(),
            "clientUserMessageId": client_message_id.as_str(),
            "input": [{"type": "text", "text": prompt.as_str()}],
            "approvalPolicy": policy.approval,
            "sandboxPolicy": {
                "type": policy.sandbox,
                "networkAccess": policy.network_access
            }
        });
        if let Some(model) = settings.model {
            params["model"] = json!(model.as_str());
        }
        if let Some(effort) = settings.effort {
            params["effort"] = json!(effort.as_str());
        }
        Self::build(id, RequestKind::TurnStart, params)
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

    pub(crate) fn steer(
        id: u64,
        thread_id: &RequiredText<512>,
        turn_id: &RequiredText<512>,
        prompt: &Prompt,
        client_message_id: &RequiredText<512>,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::Steer,
            json!({
                "threadId": thread_id.as_str(),
                "expectedTurnId": turn_id.as_str(),
                "clientUserMessageId": client_message_id.as_str(),
                "input": [{"type": "text", "text": prompt.as_str()}]
            }),
        )
    }

    pub(crate) fn fork_release(
        id: u64,
        thread_id: &str,
        delete: bool,
    ) -> Result<Self, RequestError> {
        let kind = if delete {
            RequestKind::ForkDelete
        } else {
            RequestKind::ForkRelease
        };
        Self::build(id, kind, json!({"threadId": thread_id}))
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

fn codex_dynamic_tools() -> Vec<Value> {
    vec![json!({
        "type": "function",
        "name": CODEX_BROWSE_TOOL_NAME,
        "description": "Open a Sage Preview URL. Only loopback HTTP(S) URLs with an explicit port are allowed.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "required": ["url"],
            "properties": {
                "url": {"type": "string", "maxLength": MAX_HARNESS_BROWSE_URL_BYTES}
            }
        }
    })]
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
    #[error("the selected access mode is not admitted by this Codex binding")]
    UnsupportedAccessMode,
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
