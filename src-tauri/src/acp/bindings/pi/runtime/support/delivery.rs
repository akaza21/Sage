use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressResolution, EgressToken},
    transport::OutboundFrame,
};

use super::super::{DeliveryPhase, DenyStage, PiRuntime, RequestKind};
use crate::acp::bindings::pi::Operation;

impl PiRuntime {
    pub(in crate::acp::bindings::pi::runtime) fn rollback(
        &mut self,
        id: u64,
    ) -> Result<(), AdapterRuntimeError> {
        let reservation = self
            .reservations
            .remove(&id)
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        self.translator
            .reject_before_write(&reservation.command_id)
            .map_err(super::map_pi)?;
        match reservation.kind {
            RequestKind::InitializeFirst | RequestKind::InitializeSecond => {
                self.staged_initialize = None;
                self.initialized = false;
            }
            RequestKind::Commands => self.commands_pending = false,
            RequestKind::Prompt => self.active_turn = None,
            RequestKind::Steer => self.rollback_steer(id),
            RequestKind::ThinkingLevels => {
                self.clear_thinking_request();
            }
            // Never written, so its caller is answered by the write failure.
            RequestKind::Native => self.native_pending = None,
            RequestKind::Models
            | RequestKind::RefreshState
            | RequestKind::SetModel
            | RequestKind::SetThinkingLevel
            | RequestKind::NewSession
            | RequestKind::ResumeSession
            | RequestKind::ClearQueue
            | RequestKind::Cancel => {}
        }
        Ok(())
    }

    /// Settles a request's reservation once its frame was queued, written
    /// or refused.
    pub(in crate::acp::bindings::pi::runtime) fn resolve_request(
        &mut self,
        token: &EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        let id = token
            .request_id()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        match resolution {
            EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes => self.rollback(id),
            EgressResolution::Queued => {
                let reservation = self
                    .reservations
                    .get_mut(&id)
                    .ok_or(AdapterRuntimeError::EgressIntent)?;
                reservation.phase = DeliveryPhase::Queued;
                Ok(())
            }
            EgressResolution::Written => {
                let reservation = self
                    .reservations
                    .get(&id)
                    .ok_or(AdapterRuntimeError::EgressIntent)?;
                if reservation.phase != DeliveryPhase::Queued
                    && !matches!(
                        reservation.kind,
                        RequestKind::SetModel
                            | RequestKind::SetThinkingLevel
                            | RequestKind::Commands
                            | RequestKind::Native
                    )
                {
                    return Err(AdapterRuntimeError::EgressIntent);
                }
                if reservation.kind == RequestKind::Prompt {
                    self.translator
                        .mark_prompt_written(&reservation.command_id)
                        .map_err(super::map_pi)?;
                }
                self.reservations.remove(&id);
                Ok(())
            }
            EgressResolution::DeliveryUnknown => {
                let reservation = self
                    .reservations
                    .remove(&id)
                    .ok_or(AdapterRuntimeError::EgressIntent)?;
                // No answer will come for it, so its caller stops waiting.
                if self.native_pending.as_ref() == Some(&reservation.command_id) {
                    self.native_pending = None;
                }
                self.staged_initialize = None;
                self.poisoned = true;
                Ok(())
            }
        }
    }

    pub(in crate::acp::bindings::pi::runtime) fn snapshot(
        &self,
    ) -> crate::acp::intent::CapabilitySnapshotV1 {
        super::capabilities(
            self.model_count,
            self.model_session_config()
                .map_or(0, |config| config.config_options.as_slice().len() as u16),
        )
    }

    pub(super) fn mandatory_abort(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let deny = self
            .deny
            .as_ref()
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        if !deny.command_id.is_empty() {
            return Ok(None);
        }
        let mut outbound = self
            .translator
            .begin(Operation::Abort)
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
        if outbound.len() != 1 {
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        self.translator.latch_authority_denial();
        let outbound = outbound.remove(0);
        let frame = OutboundFrame::new(outbound.line.into_bytes())
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
        let deny = self
            .deny
            .as_mut()
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        deny.command_id = outbound.command_id;
        Ok(Some(AdapterEgress::new(
            frame,
            EgressToken::mandatory_deny(deny.request_id.clone(), false),
        )))
    }

    pub(crate) fn commit_deny(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        let (request, replay) = token
            .deny_request()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if replay {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        let deny = self
            .deny
            .as_mut()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if deny.request_id.as_str() != request {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        match deny.stage {
            DenyStage::Prepared => deny.stage = DenyStage::Committed,
            DenyStage::Committed | DenyStage::Queued | DenyStage::Written => {}
        }
        Ok(())
    }

    pub(crate) fn resolve_deny(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        let (request, replay) = token
            .deny_request()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if replay {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        if matches!(
            resolution,
            EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown
        ) {
            self.poison();
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let deny = self
            .deny
            .as_mut()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if deny.request_id.as_str() != request {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        match (deny.stage, resolution) {
            (DenyStage::Committed, EgressResolution::Queued) => deny.stage = DenyStage::Queued,
            (DenyStage::Queued, EgressResolution::Written) if deny.releases => self.deny = None,
            (DenyStage::Queued, EgressResolution::Written) => deny.stage = DenyStage::Written,
            (DenyStage::Queued, EgressResolution::Queued)
            | (DenyStage::Written, EgressResolution::Written) => {}
            _ => return Err(AdapterRuntimeError::EgressIntent),
        }
        Ok(())
    }

    pub(super) fn poison(&mut self) {
        self.poisoned = true;
        self.staged_initialize = None;
    }

    pub(super) fn batch_failure(
        &mut self,
        error: AdapterRuntimeError,
        mandatory: bool,
        staged_id: Option<u64>,
    ) -> AdapterRuntimeError {
        if let Some(id) = staged_id {
            let _ = self.rollback(id);
        }
        if mandatory {
            self.poison();
            AdapterRuntimeError::MandatoryDenyTeardown
        } else {
            error
        }
    }
}
