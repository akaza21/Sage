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

use std::{borrow::Cow, collections::VecDeque};

use serde_json::{Value, json};

use super::{BridgeError, Inbound, MAX_PENDING, Pending, SdkBridge};

/// What a stop still sends Claude Code, in order.
#[derive(Debug)]
enum StopStep {
    Withdraw(String),
    Interrupt,
}

/// The stop in progress: what it still sends, and the requests it sent.
#[derive(Debug, Default)]
pub(super) struct Stop {
    plan: VecDeque<StopStep>,
    sent: Vec<String>,
    /// Its latest interrupt, so a run gets one interrupt, not two.
    interrupt: Option<String>,
}

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

    /// Stopping withdraws the prompt Claude Code has not taken up, then each
    /// waiting message, then interrupts. A build that reports no lifecycle
    /// never says whether a run took the prompt up, so its prompt is
    /// withdrawn only when stamped to be queued again, which an interrupt
    /// alone would put back.
    pub(super) fn stop(&mut self) -> Result<Value, BridgeError> {
        let stamped = self.requeuable.take().is_some();
        let prompt = (stamped || self.lifecycles)
            .then(|| self.waiting_prompt.clone())
            .flatten();
        self.stop.plan = prompt
            .into_iter()
            .chain(self.steers.iter().cloned())
            .map(StopStep::Withdraw)
            .chain([StopStep::Interrupt])
            .collect();
        self.next_stop_step()
    }

    fn next_stop_step(&mut self) -> Result<Value, BridgeError> {
        let (request, pending) = match self.stop.plan.front() {
            Some(StopStep::Withdraw(uuid)) => (
                json!({"subtype": "cancel_async_message", "message_uuid": uuid}),
                Pending::Withdraw(uuid.clone()),
            ),
            Some(StopStep::Interrupt) => (json!({"subtype": "interrupt"}), Pending::Interrupt),
            None => return Err(BridgeError::Unsupported),
        };
        let interrupt = matches!(pending, Pending::Interrupt);
        // A step Sage has no room to send yet stays first, for the next frame.
        let sdk = self.control(request, pending)?;
        self.stop.plan.pop_front();
        let id = sdk["request_id"].as_str().unwrap_or_default().to_owned();
        if interrupt {
            self.stop.interrupt = Some(id.clone());
        }
        self.stop.sent.push(id);
        Ok(sdk)
    }

    /// Moves a stop on by one step after each frame Claude Code sends, so no
    /// withdrawal waits on another's reply; Claude Code takes Sage's requests
    /// in order, so each lands before the interrupt. With no run active, the
    /// interrupt waits until each message this stop withdrew is settled, by
    /// its reply or its lifecycle frame, and a stop that leaves nothing to
    /// run ends the turn instead of interrupting an idle Claude Code.
    pub(super) fn continue_stop(&mut self, inbound: &mut Inbound) {
        if !self.interrupted || self.prompt.is_none() || self.stop.plan.is_empty() {
            return;
        }
        let idle = !self.running || self.declining;
        if idle && matches!(self.stop.plan.front(), Some(StopStep::Interrupt)) {
            if self.withdrawing() {
                return;
            }
            if let Some(held) = self.release_held() {
                self.stop.plan.clear();
                self.result(&held, inbound);
                return;
            }
        }
        if let Ok(next) = self.next_stop_step() {
            inbound.replies.push(next);
        }
    }

    /// A message this stop withdrew still waits, its withdrawal unanswered.
    fn withdrawing(&self) -> bool {
        self.stop
            .sent
            .iter()
            .filter_map(|id| match self.pending.get(id) {
                Some(Pending::Withdraw(uuid)) => Some(uuid),
                _ => None,
            })
            .any(|uuid| self.steers.contains(uuid) || self.waiting_prompt.as_deref() == Some(uuid))
    }

    /// A run that takes the prompt up while stopping gets an interrupt,
    /// unless the stop's own still lies ahead or has not been answered yet:
    /// Claude Code reports the run before taking that interrupt, which ends it.
    fn interrupt_started_run(&mut self, inbound: &mut Inbound) {
        let ahead = self
            .stop
            .plan
            .iter()
            .any(|step| matches!(step, StopStep::Interrupt));
        let unanswered = self
            .stop
            .interrupt
            .as_ref()
            .is_some_and(|id| self.pending.contains_key(id));
        if ahead || unanswered {
            return;
        }
        self.stop.plan = VecDeque::from([StopStep::Interrupt]);
        if let Ok(interrupt) = self.next_stop_step() {
            inbound.replies.push(interrupt);
        }
    }

    /// A stop's requests still unanswered when its turn ends are forgotten,
    /// so they hold no capacity and their late replies are dropped.
    fn forget_stop(&mut self) {
        let stop = std::mem::take(&mut self.stop);
        for id in stop.sent {
            if self.pending.remove(&id).is_some() {
                if self.forgotten.len() >= MAX_PENDING {
                    self.forgotten.pop_front();
                }
                self.forgotten.push_back(id);
            }
        }
    }

    /// What a reply from Claude Code answers; a forgotten request's reply answers nothing.
    pub(super) fn answered_request(&mut self, request_id: &str) -> Result<Pending, BridgeError> {
        if let Some(pending) = self.pending.remove(request_id) {
            return Ok(pending);
        }
        let forgotten = self.forgotten.iter().position(|id| id == request_id);
        forgotten
            .and_then(|index| self.forgotten.remove(index))
            .map(|_| Pending::Abandoned)
            .ok_or(BridgeError::Frame)
    }

    /// Claude Code's reply to a withdrawal. A message no longer waits on Sage
    /// whatever it was; the prompt only once Claude Code cancelled it, since
    /// otherwise a run already took it up and the interrupt ends that run.
    /// A reply after its turn ended changes nothing.
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
                if self.interrupted {
                    self.interrupt_started_run(inbound);
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
        if self.running && !self.declining {
            return;
        }
        if !self.steers.is_empty() {
            self.held.get_or_insert(Value::Null);
            return;
        }
        let end = self.held.take().unwrap_or(Value::Null);
        self.result(&end, inbound);
    }

    /// What a run's `result` ends the open turn with, if it ends it: for a
    /// prompt Claude Code would not run, the run's own `result` reporting
    /// that failure, so its usage and context window still show.
    pub(super) fn turn_end<'a>(&mut self, sdk: &'a Value) -> Option<Cow<'a, Value>> {
        if self.prompt.is_none() || self.holds_result(sdk) {
            return None;
        }
        let Some(message) = self.prompt_failure.take() else {
            return Some(Cow::Borrowed(sdk));
        };
        let mut end = sdk.clone();
        end["is_error"] = true.into();
        end["result"] = message.into();
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
        self.forget_stop();
        for uuid in std::mem::take(&mut self.steers) {
            inbound.steers.push((uuid, false));
        }
    }
}
