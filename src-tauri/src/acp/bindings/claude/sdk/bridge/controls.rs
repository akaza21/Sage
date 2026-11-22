use serde_json::{Value, json};

use super::{
    BridgeError, Inbound, Pending, SdkBridge,
    notices::{TASK_PREFIX, prose},
};
use crate::acp::{goal::NativeRpcReply, mcp::inventory::McpInventory, turn_undo::MAX_REWIND_FILES};
use sage_acp_protocol::{contract::SafeText, session_control::SessionControl};

/// Passive requests Claude Code has not answered after their callers gave
/// up; at this many, further passive requests wait until one is answered.
const MAX_ABANDONED_REQUESTS: usize = 4;

/// A request that changes nothing Sage holds, so its caller may give up on
/// it (see `SessionControl::abandons_at_deadline`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PassiveRequest {
    McpList,
    McpReconnect,
    Usage,
    /// A `rewind_files` dry run: it lists files and restores none.
    FilePlan,
}

impl PassiveRequest {
    pub(super) fn answered(self, reply: Result<&Value, &str>, inbound: &mut Inbound) {
        match (self, reply) {
            (Self::McpList, Ok(reply)) => SdkBridge::mcp_listed(reply, inbound),
            (Self::Usage, reply) => super::usage::answered(reply.ok(), inbound),
            (Self::FilePlan, Ok(reply)) => file_plan(reply, inbound),
            (Self::McpList | Self::McpReconnect | Self::FilePlan, reply) => {
                SdkBridge::controlled(reply, inbound)
            }
        }
    }
}

/// `filesChanged` names the files a dry run would restore, or a restore did.
pub(super) fn file_plan(reply: &Value, inbound: &mut Inbound) {
    if reply.get("canRewind").and_then(Value::as_bool) != Some(true) {
        return SdkBridge::controlled(Ok(reply), inbound);
    }
    let files = match reply.get("filesChanged") {
        None => Some(Vec::new()),
        Some(Value::Array(files)) if files.len() <= MAX_REWIND_FILES => files
            .iter()
            .map(|file| {
                file.as_str()
                    .filter(|path| !path.is_empty())
                    .map(Into::into)
            })
            .collect(),
        Some(Value::Array(_)) => {
            inbound.native = Some(NativeRpcReply::TooLarge);
            return;
        }
        Some(_) => None,
    };
    inbound.native = Some(files.map_or(NativeRpcReply::Rejected, NativeRpcReply::FileRewindPlan));
}

impl SdkBridge {
    pub(in crate::acp::bindings::claude::sdk) fn session_control(
        &mut self,
        control: &SessionControl,
    ) -> Result<Value, BridgeError> {
        if self.controlling() || (control.abandons_at_deadline() && self.abandoned_full()) {
            return Err(BridgeError::Capacity);
        }
        let mut pending = Pending::Control;
        let request = match control {
            SessionControl::RewindConversation { .. } => json!({"subtype":"get_settings"}),
            SessionControl::ForkAt { .. } => return Err(BridgeError::Unsupported),
            SessionControl::UndoTurn { turn_id } => json!({
                "subtype": "rewind_files",
                "user_message_id": message_uuid(turn_id.as_str()).ok_or(BridgeError::Unsupported)?,
            }),
            // Claude Code checkpoints files at each prompt it was sent, under
            // the prompt's `uuid`: Sage's turn id.
            SessionControl::RewindFiles { target, dry_run } => {
                pending = if *dry_run {
                    Pending::Passive(PassiveRequest::FilePlan)
                } else {
                    Pending::Restore
                };
                json!({
                    "subtype": "rewind_files",
                    "user_message_id": message_uuid(target.turn_id.as_str())
                        .ok_or(BridgeError::Unsupported)?,
                    "dry_run": dry_run,
                })
            }
            SessionControl::StopActivity { activity_id } => {
                let task = activity_id
                    .as_str()
                    .strip_prefix(TASK_PREFIX)
                    .ok_or(BridgeError::Unsupported)?;
                json!({"subtype": "stop_task", "task_id": task})
            }
            SessionControl::ReadMcpServers => {
                pending = Pending::Passive(PassiveRequest::McpList);
                json!({"subtype": "mcp_status"})
            }
            SessionControl::ReconnectMcpServer { name } => {
                pending = Pending::Passive(PassiveRequest::McpReconnect);
                json!({"subtype": "mcp_reconnect", "serverName": name.as_str()})
            }
            // Claude Code reloads its MCP configuration only together with
            // its plugins, and signs in to servers only in its own `/mcp`.
            SessionControl::SetTitle { .. }
            | SessionControl::ReloadMcpServers
            | SessionControl::SignInMcpServer { .. } => return Err(BridgeError::Unsupported),
        };
        self.control(request, pending)
    }

    pub(in crate::acp::bindings::claude::sdk) fn usage_read(
        &mut self,
    ) -> Result<Value, BridgeError> {
        if self.controlling() || self.abandoned_full() {
            return Err(BridgeError::Capacity);
        }
        self.control(
            super::usage::read_request(),
            Pending::Passive(PassiveRequest::Usage),
        )
    }

    fn abandoned_full(&self) -> bool {
        self.pending
            .values()
            .filter(|pending| matches!(pending, Pending::Abandoned))
            .count()
            >= MAX_ABANDONED_REQUESTS
    }

    pub(in crate::acp::bindings::claude::sdk) fn controlling(&self) -> bool {
        self.pending.values().any(|pending| {
            matches!(
                pending,
                Pending::Control
                    | Pending::Restore
                    | Pending::Passive(_)
                    | Pending::Fork(_)
                    | Pending::ForkAt(..)
            )
        })
    }

    /// Passive requests change nothing Sage holds, and Claude Code may still finish a reconnect.
    /// The request id stays known, so the late reply is dropped rather than read as a frame Sage
    /// never asked for.
    pub(in crate::acp::bindings::claude::sdk) fn abandon_passive_requests(&mut self) {
        for pending in self.pending.values_mut() {
            if matches!(pending, Pending::Passive(_)) {
                *pending = Pending::Abandoned;
            }
        }
    }

    fn mcp_listed(reply: &Value, inbound: &mut Inbound) {
        inbound.native = Some(
            McpInventory::from_claude(reply)
                .map_or(NativeRpcReply::Rejected, NativeRpcReply::McpServers),
        );
    }

    /// A fork answers once Claude Code shows it is still running; the copy
    /// itself is made when the fork's chat first launches (see `fork`).
    pub(in crate::acp::bindings::claude::sdk) fn fork(
        &mut self,
        config: Option<std::path::PathBuf>,
    ) -> Result<Value, BridgeError> {
        if self.controlling() || self.prompt.is_some() {
            return Err(BridgeError::Capacity);
        }
        self.control(json!({"subtype": "get_settings"}), Pending::Fork(config))
    }

    pub(super) fn forked(&self, config: Option<&std::path::Path>, inbound: &mut Inbound) {
        inbound.native = Some(
            config
                .and_then(|config| super::super::fork::Fork::of(&self.session_id, config))
                .and_then(|fork| {
                    sage_acp_protocol::intent::ExternalId::new("externalSessionId", fork.id()).ok()
                })
                .map_or(NativeRpcReply::Rejected, NativeRpcReply::Forked),
        );
    }

    pub(in crate::acp::bindings::claude::sdk) fn fork_at(
        &mut self,
        config: Option<std::path::PathBuf>,
        turn: sage_acp_protocol::contract::SageTurnId,
    ) -> Result<Value, BridgeError> {
        if self.controlling() || self.prompt.is_some() {
            return Err(BridgeError::Capacity);
        }
        self.control(
            json!({"subtype":"get_settings"}),
            Pending::ForkAt(config, turn),
        )
    }

    pub(super) fn forked_at(
        &self,
        config: Option<&std::path::Path>,
        turn: &str,
        inbound: &mut Inbound,
    ) {
        inbound.native = Some(
            config
                .and_then(|config| {
                    super::super::fork::Fork::before(&self.session_id, config, turn).ok()
                })
                .and_then(|fork| {
                    sage_acp_protocol::intent::ExternalId::new("externalSessionId", fork.id()).ok()
                })
                .map_or(NativeRpcReply::Rejected, NativeRpcReply::Forked),
        );
    }

    pub(super) fn controlled(reply: Result<&Value, &str>, inbound: &mut Inbound) {
        let summary = |text: Option<String>| {
            text.and_then(|text| prose(&text, 512))
                .and_then(|text| SafeText::new("controlSummary", text).ok())
        };
        inbound.native = Some(match reply {
            Ok(reply) if reply.get("canRewind").and_then(Value::as_bool) == Some(false) => {
                let reason = reply
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                NativeRpcReply::ControlRefused(summary(reason))
            }
            Ok(reply) => NativeRpcReply::Controlled(summary(
                reply
                    .get("filesChanged")
                    .and_then(Value::as_array)
                    .map(|files| match files.len() {
                        1 => "Restored 1 file".to_owned(),
                        count => format!("Restored {count} files"),
                    }),
            )),
            Err(reason) => NativeRpcReply::ControlRefused(summary(Some(reason.to_owned()))),
        });
    }
}

/// The SDK message id Sage gives a turn's prompt: the UUID in its turn id,
/// so an undo still finds it after the session is resumed.
pub(in crate::acp::bindings::claude::sdk) fn message_uuid(turn_id: &str) -> Option<String> {
    let id = turn_id.strip_prefix("turn-")?;
    uuid::Uuid::parse_str(id).ok().map(|id| id.to_string())
}
