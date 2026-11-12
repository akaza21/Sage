//! Keeps an idle park from cutting off a session control.
//!
//! A config change or native RPC (a fork, a usage read, a session control)
//! is not a Turn, so the scheduler sees its runtime as idle while the harness
//! is still mid-command. Parking it then would end the process partway: a
//! fork could leave the harness on its copy. Every such command holds the
//! gate while it waits for its answer, and an idle park claims the gate only
//! when nothing holds it. Both happen under one lock, so a command either
//! starts first and keeps the runtime, or finds the park under way and is
//! refused before anything reaches the harness.

use std::sync::{Mutex, MutexGuard, PoisonError};

use super::super::types::HostError;
use super::RuntimeHost;

#[derive(Debug, Default)]
pub(super) struct ControlGate {
    state: Mutex<GateState>,
}

#[derive(Debug, Default)]
struct GateState {
    in_flight: usize,
    parking: bool,
}

impl ControlGate {
    fn lock(&self) -> MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A session control waiting for its answer.
pub(super) struct ControlHold<'a>(&'a ControlGate);

impl Drop for ControlHold<'_> {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        state.in_flight = state.in_flight.saturating_sub(1);
    }
}

/// An idle park under way; session controls are refused until it is
/// dropped.
pub(crate) struct IdleParkClaim<'a>(&'a ControlGate);

impl Drop for IdleParkClaim<'_> {
    fn drop(&mut self) {
        self.0.lock().parking = false;
    }
}

impl RuntimeHost {
    /// Holds the gate for one session control. An idle park under way
    /// refuses it with `State`, which every caller reads as "not now".
    pub(super) fn hold_control(&self) -> Result<ControlHold<'_>, HostError> {
        let mut state = self.controls.lock();
        if state.parking {
            return Err(HostError::State);
        }
        state.in_flight += 1;
        Ok(ControlHold(&self.controls))
    }

    /// Whether no session control is waiting for its answer.
    pub(crate) fn controls_settled(&self) -> bool {
        self.controls.lock().in_flight == 0
    }

    /// Claims the runtime for an idle park, or None while a session control
    /// is waiting for its answer or another park holds it.
    pub(crate) fn claim_idle_park(&self) -> Option<IdleParkClaim<'_>> {
        let mut state = self.controls.lock();
        if state.in_flight > 0 || state.parking {
            return None;
        }
        state.parking = true;
        Some(IdleParkClaim(&self.controls))
    }
}
