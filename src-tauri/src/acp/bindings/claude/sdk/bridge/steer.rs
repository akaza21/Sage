//! Claude Code queues a `user` message that arrives mid-run and reports it
//! by `command_lifecycle` frames under the message's `uuid`: `queued`, then
//! `started` when a run takes it up (at the next tool boundary of the
//! running one, or as a run of its own right after), then `completed` or
//! `cancelled`. A run that ends while such a message still waits is not
//! the end of the turn: the message runs next, and its `result` ends it.
//! Stopping withdraws the waiting messages first (`cancel_async_message`),
//! since an interrupt alone lets them run afterwards.
//!
//! The prompt itself is reported the same way. A run that starts before
//! Claude Code takes it up is one Claude Code started on its own (after a
//! background task ended), so its `result` is held like a waiting message's:
//! the run that takes the prompt up ends the turn.

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
    /// rest one by one as each reply arrives, then interrupt. A prompt
    /// stamped to be queued again goes first: once Claude Code has answered
    /// its withdrawal, no interrupt puts it back.
    pub(super) fn withdraw_steers(&mut self) -> Result<Value, BridgeError> {
        let next = self
            .requeuable
            .take()
            .or_else(|| self.steers.first().cloned());
        let Some(uuid) = next else {
            return self.control(json!({"subtype": "interrupt"}), Pending::Interrupt);
        };
        self.control(
            json!({"subtype": "cancel_async_message", "message_uuid": uuid}),
            Pending::Withdraw(uuid),
        )
    }

    /// Claude Code's reply to a withdrawal, whatever it was: the message no
    /// longer waits on Sage, and the stop moves on.
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
        self.lifecycles = true;
        let state = sdk.get("state").and_then(Value::as_str);
        if self.waiting_prompt.as_deref() == Some(uuid) {
            self.prompt_lifecycle(state, inbound);
            return;
        }
        if !self.steers.iter().any(|steer| steer == uuid) {
            return;
        }
        match state {
            Some("queued") => inbound.steers.push((uuid.to_owned(), true)),
            Some("started") => {
                inbound.steers.push((uuid.to_owned(), true));
                self.steers.retain(|steer| steer != uuid);
                // The run that takes it up reports its own end.
                if self.held.take().is_some() {
                    self.streamed.clear();
                }
            }
            Some("completed" | "cancelled" | "discarded" | "refused") => {
                inbound.steers.push((uuid.to_owned(), false));
                self.steers.retain(|steer| steer != uuid);
                if let Some(held) = self.release_held() {
                    self.result(&held, inbound);
                }
            }
            _ => {}
        }
    }

    /// Claude Code took up the prompt, or will never run it. A run that
    /// takes it up reports the turn's end, even one Sage was declining, into
    /// which Claude Code folded it; a stop sent before it ran stops that run.
    /// A declined run that takes up a prompt stamped to be queued again is
    /// still stopped by Sage's interrupt, and the prompt runs after it.
    fn prompt_lifecycle(&mut self, state: Option<&str>, inbound: &mut Inbound) {
        match state {
            Some("started") if self.declining && self.requeuable.is_some() => {}
            Some("started") => {
                self.waiting_prompt = None;
                self.declining = false;
                if self.held.take().is_some() {
                    self.streamed.clear();
                }
                if self.interrupted
                    && let Ok(interrupt) =
                        self.control(json!({"subtype": "interrupt"}), Pending::Interrupt)
                {
                    inbound.replies.push(interrupt);
                }
            }
            Some(end @ ("completed" | "cancelled" | "discarded" | "refused")) => {
                self.waiting_prompt = None;
                self.prompt_ended(end, inbound);
            }
            _ => {}
        }
    }

    /// No run will take the prompt up. The turn ends here unless a run of
    /// its own, or a waiting message's, still reports its end.
    fn prompt_ended(&mut self, end: &str, inbound: &mut Inbound) {
        let failure = match end {
            "refused" => Some("Claude Code refused this message."),
            "discarded" => Some("Claude Code ended the session before running this message."),
            "cancelled" if !self.interrupted => {
                Some("Claude Code cancelled this message before answering it.")
            }
            _ => None,
        };
        if failure.is_some() {
            self.held = None;
        }
        if !self.steers.is_empty() || (self.running && !self.declining) {
            return;
        }
        let end = self.held.take().unwrap_or_else(|| {
            failure.map_or(
                Value::Null,
                |message| json!({"type": "result", "is_error": true, "result": message}),
            )
        });
        self.result(&end, inbound);
    }

    /// While stopping, a message still waits only until its withdrawal is answered; if Claude Code
    /// took it up first, the interrupt that follows ends that run.
    pub(super) fn holds_result(&mut self, sdk: &Value) -> bool {
        if !self.lifecycles || (self.steers.is_empty() && self.waiting_prompt.is_none()) {
            return false;
        }
        self.held = Some(sdk.clone());
        true
    }

    fn release_held(&mut self) -> Option<Value> {
        if self.steers.is_empty() && self.waiting_prompt.is_none() {
            self.held.take()
        } else {
            None
        }
    }

    pub(super) fn end_steers(&mut self, inbound: &mut Inbound) {
        self.held = None;
        self.waiting_prompt = None;
        self.requeuable = None;
        for uuid in std::mem::take(&mut self.steers) {
            inbound.steers.push((uuid, false));
        }
    }
}
