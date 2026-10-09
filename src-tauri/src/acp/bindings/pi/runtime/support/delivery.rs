use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressResolution, EgressToken},
    transport::OutboundFrame,
};

use super::super::{DenyStage, PiRuntime, RequestKind};
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
            RequestKind::Prompt => self.active_turn = None,
            RequestKind::RefreshState => self.refresh_thinking_after_model = false,
            RequestKind::ThinkingLevels => {
                self.clear_thinking_request();
            }
            RequestKind::Models
            | RequestKind::SetModel
            | RequestKind::SetThinkingLevel
            | RequestKind::NewSession
            | RequestKind::Cancel => {}
        }
        Ok(())
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
