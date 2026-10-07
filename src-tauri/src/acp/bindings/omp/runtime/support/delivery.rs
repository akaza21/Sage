use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressResolution, EgressToken},
    intent::ExternalId,
    limits::LIMITS,
};

use super::super::{DenyIntent, DenyStage, OmpRuntime, RequestKind};
use crate::acp::bindings::omp::{AdapterEvent, Outbound};

impl OmpRuntime {
    pub(in crate::acp::bindings::omp::runtime) fn reject_outbound(
        &mut self,
        outbound: &Outbound,
        kind: RequestKind,
    ) -> Result<(), AdapterRuntimeError> {
        match outbound.request_id.as_deref() {
            Some(id) => self
                .translator
                .reject_before_write(id)
                .map_err(super::map_omp),
            None if matches!(kind, RequestKind::Cancel | RequestKind::AuthorityCancel) => self
                .translator
                .reject_cancel_before_write()
                .map_err(super::map_omp),
            None => Err(AdapterRuntimeError::EgressIntent),
        }
    }

    pub(super) fn prepare_deny(
        &mut self,
        request_id: ExternalId,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        if self.denials.contains_key(request_id.as_str()) {
            return Ok(None);
        }
        if self.denials.len() >= LIMITS.queues.server_requests_per_session as usize {
            self.poison();
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let (outbound, replay) = self
            .translator
            .permission_decline_prepared(request_id.as_str())
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
        let frame = super::outbound_frame(&outbound.line)
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
        self.denials.insert(
            request_id.as_str().to_owned(),
            DenyIntent {
                replay,
                cancelled: false,
                stage: if replay {
                    DenyStage::Committed
                } else {
                    DenyStage::Prepared
                },
            },
        );
        Ok(Some(AdapterEgress::new(
            frame,
            EgressToken::mandatory_deny(request_id, replay),
        )))
    }

    pub(in crate::acp::bindings::omp::runtime) fn commit_deny(
        &mut self,
        token: &EgressToken,
    ) -> Result<(), AdapterRuntimeError> {
        let (request_id, replay, cancelled) = token
            .deny_request()
            .map(|(id, replay)| (id, replay, false))
            .or_else(|| {
                token
                    .cancelled_permission_request()
                    .map(|id| (id, false, true))
            })
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        let intent = self
            .denials
            .get_mut(request_id)
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if intent.replay != replay || intent.cancelled != cancelled {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        match intent.stage {
            DenyStage::Prepared if !replay => {
                if cancelled {
                    self.translator
                        .permission_cancel_committed(request_id)
                        .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
                } else {
                    let response = self
                        .translator
                        .permission_decline_committed(request_id)
                        .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
                    if matches!(response.event, AdapterEvent::Item { .. }) {
                        self.deferred.push(response.event);
                    }
                }
                intent.stage = DenyStage::Committed;
                Ok(())
            }
            DenyStage::Committed | DenyStage::Queued => Ok(()),
            DenyStage::Prepared => Err(AdapterRuntimeError::EgressIntent),
        }
    }

    pub(in crate::acp::bindings::omp::runtime) fn resolve_deny(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        let (request_id, replay, cancelled) = token
            .deny_request()
            .map(|(id, replay)| (id, replay, false))
            .or_else(|| {
                token
                    .cancelled_permission_request()
                    .map(|id| (id, false, true))
            })
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if matches!(
            resolution,
            EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown
        ) {
            self.denials.remove(request_id);
            self.poison();
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let intent = self
            .denials
            .get_mut(request_id)
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if intent.replay != replay || intent.cancelled != cancelled {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        match (intent.stage, resolution) {
            (DenyStage::Committed, EgressResolution::Queued) => {
                intent.stage = DenyStage::Queued;
                Ok(())
            }
            (DenyStage::Queued, EgressResolution::Written) => {
                self.denials.remove(request_id);
                Ok(())
            }
            _ => Err(AdapterRuntimeError::EgressIntent),
        }
    }

    pub(in crate::acp::bindings::omp::runtime) fn prepare_permission_cancel(
        &mut self,
        request_id: &crate::acp::contract::SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let external = ExternalId::new("permissionRequest", request_id.as_str())
            .map_err(|_| AdapterRuntimeError::PermissionIntent)?;
        let outbound = self
            .translator
            .permission_cancel_prepared(external.as_str())
            .map_err(|_| AdapterRuntimeError::PermissionIntent)?;
        let frame = super::outbound_frame(&outbound.line)
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
        match self.denials.get_mut(external.as_str()) {
            Some(intent) if intent.stage == DenyStage::Prepared && !intent.replay => {
                intent.cancelled = true;
            }
            Some(_) => return Err(AdapterRuntimeError::PermissionIntent),
            None => {
                self.denials.insert(
                    external.as_str().to_owned(),
                    DenyIntent {
                        replay: false,
                        cancelled: true,
                        stage: DenyStage::Prepared,
                    },
                );
            }
        }
        Ok(AdapterEgress::new(
            frame,
            EgressToken::permission_cancel(external),
        ))
    }
}
