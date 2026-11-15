//! Claude Code also runs on its own: after a background task ends it reports
//! the task, then runs the main agent on the report, ending on a `result`.
//! A main-agent frame with no turn open starts such a run, and the session
//! opens a turn for it.

use serde_json::Value;

use super::{BridgeError, Inbound, SdkBridge};

impl SdkBridge {
    /// A main-agent frame means a run is in progress, and one with no turn
    /// open starts a turn. A `result` ends the run.
    pub(super) fn observe_run(&mut self, nested: bool, kind: Option<&str>, inbound: &mut Inbound) {
        match kind {
            _ if nested => {}
            Some("stream_event" | "assistant" | "user") => {
                inbound.started = self.prompt.is_none();
                self.running = true;
            }
            Some("result") => self.running = false,
            _ => {}
        }
    }

    /// Names the turn Claude Code started on its own: its `result` answers `id`.
    pub(in crate::acp::bindings::claude::sdk) fn adopt(
        &mut self,
        id: Value,
    ) -> Result<(), BridgeError> {
        if self.prompt.is_some() {
            return Err(BridgeError::Frame);
        }
        self.prompt = Some(id);
        self.interrupted = false;
        Ok(())
    }

    /// A turn opened only for an approval has no run an interrupt would end
    /// with a `result`, so the stop ends it here, declining what it asked.
    pub(super) fn interrupted_idle(&mut self, inbound: &mut Inbound) {
        if !self.interrupted || self.running || self.prompt.is_none() {
            return;
        }
        self.decline_open(inbound);
        self.result(&Value::Null, inbound);
    }
}
