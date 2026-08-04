use super::*;

impl ClaudeRuntime {
    pub(super) fn commit_mandatory_egress_inner(
        &mut self,
        token: &EgressToken,
    ) -> Result<(), AdapterRuntimeError> {
        let Some((request_id, replay)) = token.deny_request() else {
            return Err(AdapterRuntimeError::EgressIntent);
        };
        if replay {
            return self
                .denials
                .get(request_id)
                .is_some_and(|record| record.state == DenialState::Written)
                .then_some(())
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let record = self
            .denials
            .get_mut(request_id)
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        if record.state == DenialState::Pending {
            let active = self
                .active
                .as_mut()
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
            active
                .translator
                .permission_decline_committed(&record.translator_id)
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
            record.state = DenialState::Committed;
        }
        matches!(
            record.state,
            DenialState::Committed | DenialState::Queued | DenialState::Written
        )
        .then_some(())
        .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)
    }

    pub(super) fn resolve_egress_inner(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if let Some(request_id) = token.request_id() {
            if self.delivery_unknown.is_some() || !self.pending.contains_key(&request_id) {
                return Err(AdapterRuntimeError::EgressIntent);
            }
            if matches!(
                resolution,
                EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes
            ) {
                let pending = self
                    .pending
                    .remove(&request_id)
                    .ok_or(AdapterRuntimeError::EgressIntent)?;
                if matches!(pending, PendingRequest::Prompt) {
                    let active = self
                        .active
                        .as_mut()
                        .ok_or(AdapterRuntimeError::EgressIntent)?;
                    active
                        .translator
                        .reject_prompt(&request_id.to_string())
                        .map_err(map_translation)?;
                    active.turn = None;
                }
            } else if resolution == EgressResolution::DeliveryUnknown {
                if !self.pending.contains_key(&request_id) {
                    return Err(AdapterRuntimeError::EgressIntent);
                }
                self.delivery_unknown = Some(DeliveryUnknown::Request(request_id));
            }
            return Ok(());
        }
        if token.is_notification() {
            if resolution == EgressResolution::DeliveryUnknown {
                self.delivery_unknown = Some(DeliveryUnknown::Notification);
            }
            return Ok(());
        }
        let Some((request_id, replay)) = token.deny_request() else {
            return Err(AdapterRuntimeError::EgressIntent);
        };
        if matches!(
            resolution,
            EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown
        ) {
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        if replay {
            return self
                .denials
                .get(request_id)
                .is_some_and(|record| record.state == DenialState::Written)
                .then_some(())
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let record = self
            .denials
            .get_mut(request_id)
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        record.state = match (resolution, record.state) {
            (EgressResolution::Queued, DenialState::Committed | DenialState::Queued) => {
                DenialState::Queued
            }
            (EgressResolution::Queued, DenialState::Written) => DenialState::Written,
            (EgressResolution::Written, DenialState::Queued | DenialState::Written) => {
                DenialState::Written
            }
            _ => return Err(AdapterRuntimeError::MandatoryDenyTeardown),
        };
        Ok(())
    }
}
