use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, AdapterRuntimeError, EgressToken},
        pi::{Operation, Outbound},
    },
    limits::LIMITS,
    transport::OutboundFrame,
};

use super::super::{DeliveryPhase, PiRuntime, RequestKind, RequestReservation};

impl PiRuntime {
    pub(in crate::acp::bindings::pi::runtime) fn begin_one(
        &mut self,
        operation: Operation,
        kind: RequestKind,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let mut values = self.translator.begin(operation).map_err(super::map_pi)?;
        if values.len() != 1 {
            return Err(AdapterRuntimeError::Operation);
        }
        self.reserve(values.remove(0), kind)
    }

    pub(in crate::acp::bindings::pi::runtime) fn begin_initialize(
        &mut self,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.initialized || self.staged_initialize.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        let mut values = self
            .translator
            .begin(Operation::Initialize)
            .map_err(super::map_pi)?;
        if values.len() != 2 {
            return Err(AdapterRuntimeError::Operation);
        }
        let second = values.pop().ok_or(AdapterRuntimeError::Operation)?;
        let first = values.pop().ok_or(AdapterRuntimeError::Operation)?;
        let egress = self.reserve(first, RequestKind::InitializeFirst)?;
        self.staged_initialize = Some(second);
        Ok(egress)
    }

    pub(in crate::acp::bindings::pi::runtime) fn reserve(
        &mut self,
        outbound: Outbound,
        kind: RequestKind,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        // Until a fork moves Pi back, its open session is the copy: only the
        // fork's own steps may reach it.
        if self.fork.is_some() && kind != RequestKind::Native {
            self.translator
                .reject_before_write(&outbound.command_id)
                .map_err(super::map_pi)?;
            return Err(AdapterRuntimeError::Operation);
        }
        if self.reservations.len() >= LIMITS.queues.outstanding_rpc_per_session as usize {
            self.translator
                .reject_before_write(&outbound.command_id)
                .map_err(super::map_pi)?;
            return Err(AdapterRuntimeError::BatchCount);
        }
        let bytes = outbound.line.into_bytes();
        let frame = match if kind == RequestKind::Prompt {
            OutboundFrame::new_prompt(bytes)
        } else {
            OutboundFrame::new(bytes)
        } {
            Ok(frame) => frame,
            Err(_) => {
                self.translator
                    .reject_before_write(&outbound.command_id)
                    .map_err(super::map_pi)?;
                return Err(AdapterRuntimeError::Frame);
            }
        };
        let next_egress = match self
            .next_egress
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
        {
            Some(value) => value,
            None => {
                self.translator
                    .reject_before_write(&outbound.command_id)
                    .map_err(super::map_pi)?;
                return Err(AdapterRuntimeError::Operation);
            }
        };
        self.next_egress = next_egress;
        self.reservations.insert(
            self.next_egress,
            RequestReservation {
                command_id: outbound.command_id,
                kind,
                phase: DeliveryPhase::Reserved,
            },
        );
        let token = if kind == RequestKind::Cancel {
            EgressToken::cancel_request(self.next_egress)
        } else if kind == RequestKind::Native && self.fork.is_some() {
            EgressToken::native_rpc_continuation(self.next_egress)
        } else {
            EgressToken::request(self.next_egress)
        };
        Ok(AdapterEgress::new(frame, token))
    }
}
