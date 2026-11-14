//! Claude Code queues a `user` message that arrives mid-run and reports it
//! by `command_lifecycle` frames under the message's `uuid`: `queued`, then
//! `started` when a run takes it up (at the next tool boundary of the
//! running one, or as a run of its own right after), then `completed` or
//! `cancelled`. A run that ends while such a message still waits is not
//! the end of the turn: the message runs next, and its `result` ends it.
//! Stopping withdraws the waiting messages first (`cancel_async_message`),
//! since an interrupt alone lets them run afterwards.

use serde_json::{Value, json};

use super::{BridgeError, Inbound, MAX_PENDING, Pending, SdkBridge};

impl SdkBridge {
    pub(in crate::acp::bindings::claude::sdk) fn steer(
        &mut self,
        text: &str,
    ) -> Result<(Value, String), BridgeError> {
        if self.prompt.is_none() || self.interrupted {
            return Err(BridgeError::Unsupported);
        }
        if self.steers.len() >= MAX_PENDING {
            return Err(BridgeError::Capacity);
        }
        let uuid = uuid::Uuid::new_v4().to_string();
        self.steers.push(uuid.clone());
        let user = json!({
            "type": "user",
            "session_id": self.session_id,
            "message": {"role": "user", "content": [{"type": "text", "text": text}]},
            "parent_tool_use_id": null,
            "uuid": uuid,
        });
        Ok((user, uuid))
    }

    /// A write fails before Sage reads anything more, so no run's end can be held for it.
    pub(super) fn unsteer(&mut self, uuid: &str) {
        self.steers.retain(|steer| steer != uuid);
    }

    /// Stopping with messages still waiting: withdraw the first, then the
    /// rest one by one as each reply arrives, then interrupt.
    pub(super) fn withdraw_steers(&mut self) -> Result<Value, BridgeError> {
        let Some(uuid) = self.steers.first().cloned() else {
            return self.control(json!({"subtype": "interrupt"}), Pending::Interrupt);
        };
        self.control(
            json!({"subtype": "cancel_async_message", "message_uuid": uuid}),
            Pending::Withdraw(uuid),
        )
    }

    /// A stop whose turn already ended goes no further, so a late reply never interrupts the next
    /// turn.
    pub(super) fn withdrawn(&mut self, uuid: &str, inbound: &mut Inbound) {
        if self.steers.iter().any(|steer| steer == uuid) {
            self.steers.retain(|steer| steer != uuid);
            inbound.steers.push((uuid.to_owned(), false));
        }
        if !self.interrupted || self.prompt.is_none() {
            return;
        }
        if let Some(held) = self.release_held() {
            self.result(&held, inbound);
        } else if let Ok(next) = self.withdraw_steers() {
            inbound.replies.push(next);
        }
    }

    pub(super) fn lifecycle(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let Some(uuid) = sdk.get("command_uuid").and_then(Value::as_str) else {
            return;
        };
        if !self.steers.iter().any(|steer| steer == uuid) {
            return;
        }
        match sdk.get("state").and_then(Value::as_str) {
            Some("queued") => inbound.steers.push((uuid.to_owned(), true)),
            Some("started") => {
                inbound.steers.push((uuid.to_owned(), true));
                self.steers.retain(|steer| steer != uuid);
                // The run that takes it up reports its own end.
                if self.held.take().is_some() {
                    self.streamed.clear();
                }
            }
            Some("completed" | "cancelled") => {
                inbound.steers.push((uuid.to_owned(), false));
                self.steers.retain(|steer| steer != uuid);
                if let Some(held) = self.release_held() {
                    self.result(&held, inbound);
                }
            }
            _ => {}
        }
    }

    /// While stopping, a message still waits only until its withdrawal is answered; if Claude Code
    /// took it up first, the interrupt that follows ends that run.
    pub(super) fn holds_result(&mut self, sdk: &Value) -> bool {
        if self.steers.is_empty() {
            return false;
        }
        self.held = Some(sdk.clone());
        true
    }

    fn release_held(&mut self) -> Option<Value> {
        if self.steers.is_empty() {
            self.held.take()
        } else {
            None
        }
    }

    pub(super) fn end_steers(&mut self, inbound: &mut Inbound) {
        self.held = None;
        for uuid in std::mem::take(&mut self.steers) {
            inbound.steers.push((uuid, false));
        }
    }
}
