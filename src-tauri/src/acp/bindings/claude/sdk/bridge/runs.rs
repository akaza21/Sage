//! Claude Code also runs on its own: after a background task ends it reports
//! the task, then runs the main agent on the report, from a `system/init` to
//! a `result`. Such a run with no turn open opens one, as does an approval
//! asked with none open (a background subagent's). Before Sage's first
//! prompt Claude Code runs nothing on its own, so nothing opens a turn.

use serde_json::{Value, json};

use super::{BridgeError, Inbound, Pending, SdkBridge};

impl SdkBridge {
    /// Places a main-agent frame among Claude Code's runs before it is
    /// mapped. Returns `false` for a frame of a declined run, which is
    /// dropped.
    pub(super) fn observe_run(&mut self, sdk: &Value, inbound: &mut Inbound) -> bool {
        let kind = sdk.get("type").and_then(Value::as_str);
        let starts_run = match kind {
            Some("system") => sdk.get("subtype").and_then(Value::as_str) == Some("init"),
            Some("assistant") => true,
            _ => false,
        };
        if kind == Some("system") && starts_run {
            self.requeue = sdk
                .get("capabilities")
                .and_then(Value::as_array)
                .is_some_and(|all| all.iter().any(|one| one == "interrupt_requeue_taken_v1"));
        }
        if kind == Some("result") {
            self.running = false;
        } else if starts_run {
            self.running = true;
        }
        if self.declining {
            if kind == Some("result") {
                self.declining = false;
                self.streamed.clear();
                return false;
            }
            return !starts_run && !matches!(kind, Some("stream_event" | "user"));
        }
        if starts_run {
            inbound.started = self.prompted && self.prompt.is_none();
            self.run_started(inbound.started, inbound);
            self.approval_turn = false;
        }
        true
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

    /// Refuses the turn Claude Code just started, before Sage shows it: what
    /// it asked is declined, and a run is interrupted and read to its end
    /// unseen.
    pub(in crate::acp::bindings::claude::sdk) fn decline(
        &mut self,
    ) -> Result<Inbound, BridgeError> {
        if self.prompt.is_some() {
            return Err(BridgeError::Frame);
        }
        let mut inbound = Inbound::default();
        self.deny_open(&mut inbound);
        self.tools.clear();
        self.streamed.clear();
        if !std::mem::take(&mut self.approval_turn) {
            self.declining = true;
            let interrupt = self.control(json!({"subtype": "interrupt"}), Pending::Interrupt)?;
            inbound.replies.push(interrupt);
        }
        Ok(inbound)
    }

    /// The end of a turn opened only for approvals, once they all settled:
    /// answered and delivered, or withdrawn. Asked after each frame read and
    /// once Sage's answers reach Claude Code.
    pub(in crate::acp::bindings::claude::sdk) fn settled_turn_end(&mut self) -> Option<Inbound> {
        if !self.approval_turn
            || self.prompt.is_none()
            || !self.permissions.is_empty()
            || !self.answering.is_empty()
        {
            return None;
        }
        self.approval_turn = false;
        let mut inbound = Inbound::default();
        self.result(&Value::Null, &mut inbound);
        Some(inbound)
    }

    /// A turn opened only for an approval has no run an interrupt would end
    /// with a `result`, so the stop ends it here, declining what it asked.
    pub(super) fn interrupted_idle(&mut self, inbound: &mut Inbound) {
        if !self.interrupted || !self.approval_turn || self.prompt.is_none() {
            return;
        }
        self.approval_turn = false;
        self.decline_open(inbound);
        self.result(&Value::Null, inbound);
    }
}
