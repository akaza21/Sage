use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressResolution, EgressToken},
    transport::OutboundFrame,
};

use super::super::{DenyStage, PiRuntime};
use crate::acp::bindings::pi::Operation;

impl PiRuntime {
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
