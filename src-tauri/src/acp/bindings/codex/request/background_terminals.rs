use serde_json::json;

use crate::acp::background_terminals::{MAX_BACKGROUND_TERMINALS, TerminalAction};
use sage_acp_protocol::contract::RequiredText;

use super::{OutgoingRequest, RequestError, RequestKind};

impl OutgoingRequest {
    pub(crate) fn background_terminals(
        id: u64,
        thread_id: &RequiredText<512>,
        action: &TerminalAction,
    ) -> Result<Self, RequestError> {
        let thread = thread_id.as_str();
        let (kind, params) = match action {
            TerminalAction::List => (
                RequestKind::TerminalsList,
                json!({"threadId": thread, "limit": MAX_BACKGROUND_TERMINALS}),
            ),
            TerminalAction::Stop(handle) => (
                RequestKind::TerminalsStop,
                json!({"threadId": thread, "processId": handle}),
            ),
            TerminalAction::StopAll => (RequestKind::TerminalsStopAll, json!({"threadId": thread})),
        };
        Self::build(id, kind, params)
    }
}
