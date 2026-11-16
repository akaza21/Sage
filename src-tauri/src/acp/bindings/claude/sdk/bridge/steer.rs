//! Claude Code queues a `user` message that arrives mid-run and reports it
//! by `command_lifecycle` frames under the message's `uuid`: `queued`, then
//! `started` when a run takes it up (at the next tool boundary of the
//! running one, or as a run of its own right after), then `completed` or
//! `cancelled`. A run that ends while such a message still waits is not
//! the end of the turn: the message runs next, and its `result` ends it.
//! Stopping withdraws the waiting messages and the prompt first
//! (`cancel_async_message`), since an interrupt alone lets them run afterwards.
//!
//! The prompt itself is reported the same way. A run that starts before
//! Claude Code takes it up is one Claude Code started on its own (after a
//! background task ended), so its `result` is held like a waiting message's:
//! the run that takes the prompt up ends the turn.

use std::borrow::Cow;

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

    /// Stopping: withdraw the prompt Claude Code has not taken up, then each
    /// waiting message as the reply before it arrives, then interrupt. Once
    /// Claude Code has answered a withdrawal, no interrupt puts the message
    /// back, and a build that reports no lifecycle never says the prompt ran.
    pub(super) fn stop(&mut self) -> Result<Value, BridgeError> {
        let stamped = self.requeuable.take().is_some();
        match self
            .waiting_prompt
            .clone()
            .filter(|_| stamped || self.lifecycles)
        {
            Some(uuid) => self.withdraw_message(uuid),
            None => self.withdraw_steers(),
        }
    }

    fn withdraw_steers(&mut self) -> Result<Value, BridgeError> {
        match self.steers.first().cloned() {
            Some(uuid) => self.withdraw_message(uuid),
            None => self.control(json!({"subtype": "interrupt"}), Pending::Interrupt),
        }
    }

    fn withdraw_message(&mut self, uuid: String) -> Result<Value, BridgeError> {
        self.control(
            json!({"subtype": "cancel_async_message", "message_uuid": uuid}),
            Pending::Withdraw(uuid),
        )
    }

    /// Claude Code's reply to a withdrawal. A message no longer waits on Sage
    /// whatever it was; the prompt only once Claude Code cancelled it, since
    /// otherwise a run already took it up and the interrupt ends that run.
    /// A stop whose turn already ended goes no further, so a late reply never interrupts the next
    /// turn.
    pub(super) fn withdrawn(&mut self, uuid: &str, reply: Option<&Value>, inbound: &mut Inbound) {
        let cancelled = reply.and_then(|reply| reply.get("cancelled")) == Some(&Value::Bool(true));
        if self.steers.iter().any(|steer| steer == uuid) {
            self.steers.retain(|steer| steer != uuid);
            inbound.steers.push((uuid.to_owned(), false));
        }
        if !self.interrupted || self.prompt.is_none() {
            return;
        }
        if cancelled && self.waiting_prompt.as_deref() == Some(uuid) {
            self.waiting_prompt = None;
            self.prompt_ended("cancelled", inbound);
        } else if let Some(held) = self.release_held() {
            self.result(&held, inbound);
        }
        if self.prompt.is_some()
            && let Ok(next) = self.withdraw_steers()
        {
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
    /// its own, or a waiting message's, still reports its end; a failure
    /// stands as the turn's end even so.
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
            self.prompt_failure = failure;
        }
        if !self.steers.is_empty() || (self.running && !self.declining) {
            return;
        }
        let end = self.held.take().unwrap_or(Value::Null);
        self.result(&end, inbound);
    }

    /// What a run's `result` ends the open turn with, if it ends it: for a
    /// prompt Claude Code would not run, that failure, with the run's usage.
    pub(super) fn turn_end<'a>(&mut self, sdk: &'a Value) -> Option<Cow<'a, Value>> {
        if self.prompt.is_none() || self.holds_result(sdk) {
            return None;
        }
        let Some(message) = self.prompt_failure.take() else {
            return Some(Cow::Borrowed(sdk));
        };
        let mut end = json!({"type": "result", "is_error": true, "result": message});
        if let Some(usage) = sdk.get("usage") {
            end["usage"] = usage.clone();
        }
        Some(Cow::Owned(end))
    }

    /// While stopping, a message still waits only until its withdrawal is answered; if Claude Code
    /// took it up first, the interrupt that follows ends that run.
    fn holds_result(&mut self, sdk: &Value) -> bool {
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
        self.prompt_failure = None;
        for uuid in std::mem::take(&mut self.steers) {
            inbound.steers.push((uuid, false));
        }
    }
}
