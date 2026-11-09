//! Session controls as SDK control requests: undoing a turn rewinds the
//! files Claude Code checkpointed since that turn's message, and stopping a
//! task ends one background task.

use serde_json::{Value, json};

use super::{
    BridgeError, Inbound, Pending, SdkBridge,
    notices::{TASK_PREFIX, prose},
};
use crate::acp::{contract::SafeText, goal::NativeRpcReply, session_control::SessionControl};

impl SdkBridge {
    /// The control request for a session control; one at a time.
    pub(in crate::acp::bindings::claude::sdk) fn session_control(
        &mut self,
        control: &SessionControl,
    ) -> Result<Value, BridgeError> {
        if self.controlling() {
            return Err(BridgeError::Capacity);
        }
        let request = match control {
            SessionControl::UndoTurn { turn_id } => json!({
                "subtype": "rewind_files",
                "user_message_id": message_uuid(turn_id.as_str()).ok_or(BridgeError::Unsupported)?,
            }),
            SessionControl::StopActivity { activity_id } => {
                let task = activity_id
                    .as_str()
                    .strip_prefix(TASK_PREFIX)
                    .ok_or(BridgeError::Unsupported)?;
                json!({"subtype": "stop_task", "task_id": task})
            }
        };
        self.control(request, Pending::Control)
    }

    /// A session control is waiting on Claude Code's reply.
    pub(in crate::acp::bindings::claude::sdk) fn controlling(&self) -> bool {
        self.pending
            .values()
            .any(|pending| matches!(pending, Pending::Control))
    }

    /// Claude Code's reply to a session control. A rewind it could not
    /// carry out is refused with its reason.
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
pub(super) fn message_uuid(turn_id: &str) -> Option<String> {
    let id = turn_id.strip_prefix("turn-")?;
    uuid::Uuid::parse_str(id).ok().map(|id| id.to_string())
}
