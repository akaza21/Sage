use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterEvent, AdapterRuntimeError},
    intent::Prompt,
};

use super::super::{PiRuntime, RequestKind};
use crate::acp::bindings::pi::Operation;

/// Steering within one Turn: the in-flight steer (Pi's command id and Sage's
/// egress request id) and whether any steer reached Pi.
#[derive(Default)]
pub(in crate::acp::bindings::pi::runtime) struct Steering {
    pending: Option<(String, u64)>,
    sent: bool,
}

impl PiRuntime {
    /// Sends one steer into the running model loop; a second waits for the
    /// first answer.
    pub(in crate::acp::bindings::pi::runtime) fn begin_steer(
        &mut self,
        prompt: &Prompt,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.active_turn.is_none() || self.steering.pending.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        let egress = self.begin_one(
            Operation::Steer {
                message: prompt.as_str().to_owned(),
            },
            RequestKind::Steer,
        )?;
        let request_id = egress
            .token()
            .request_id()
            .ok_or(AdapterRuntimeError::Operation)?;
        let command_id = self.reservations[&request_id].command_id.clone();
        self.steering = Steering {
            pending: Some((command_id, request_id)),
            sent: true,
        };
        Ok(egress)
    }

    pub(super) fn steer_resolved(&mut self, command_id: &str, accepted: bool) -> AdapterEvent {
        match self.steering.pending.take() {
            Some((pending, request_id)) if pending == command_id => AdapterEvent::SteerResponse {
                request_id,
                accepted,
            },
            other => {
                self.steering.pending = other;
                AdapterEvent::IgnoredLate
            }
        }
    }

    pub(super) fn rollback_steer(&mut self, request_id: u64) {
        if self
            .steering
            .pending
            .as_ref()
            .is_some_and(|(_, pending)| *pending == request_id)
        {
            self.steering.pending = None;
        }
    }

    /// Ends the Turn's steering. Pi keeps an undelivered steer across an
    /// abort or failure and would run it ahead of the next prompt, so a Turn
    /// that steered clears Pi's queue. Without capacity for that command the
    /// queue is left as is; the next steer or prompt still works.
    pub(super) fn finish_steering(&mut self) -> Option<AdapterEgress> {
        std::mem::take(&mut self.steering)
            .sent
            .then(|| {
                self.begin_one(Operation::ClearQueue, RequestKind::ClearQueue)
                    .ok()
            })
            .flatten()
    }
}
