use super::*;

impl ClaudeRuntime {
    pub(super) fn commit_mandatory_egress_inner(
        &mut self,
        token: &EgressToken,
    ) -> Result<(), AdapterRuntimeError> {
        if let Some(request_id) = token.cancelled_permission_request() {
            let record = self
                .denials
                .get_mut(request_id)
                .filter(|record| record.state == DenialState::Pending)
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
            let active = self
                .active
                .as_mut()
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
            let event = active
                .translator
                .permission_cancel_committed(&record.translator_id)
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
            if matches!(event, AdapterEvent::Item { .. }) {
                self.deferred.push(event);
            }
            record.reply = PermissionReply::Cancelled;
            record.state = DenialState::Committed;
            return Ok(());
        }
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
            let event = active
                .translator
                .permission_decline_committed(&record.translator_id)
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
            if matches!(event, AdapterEvent::Item { .. }) {
                self.deferred.push(event);
            }
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
                    self.deferred.clear();
                }
                // A delete that was never written stays wanted for the next
                // idle, up to `MAX_FORK_DELETE_ATTEMPTS` attempts.
                if let PendingRequest::DeleteFork(_, fork) = pending {
                    self.want_fork_delete(&fork);
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
            if token.is_cancel()
                && resolution == EgressResolution::Written
                && let Some(turn) = self.active.as_mut().and_then(|active| active.turn.as_mut())
            {
                turn.cancel_written = true;
            }
            if resolution == EgressResolution::DeliveryUnknown {
                self.delivery_unknown = Some(DeliveryUnknown::Notification);
            }
            return Ok(());
        }
        if let Some(request_id) = token.allow_request() {
            if matches!(
                resolution,
                EgressResolution::QueueRejected
                    | EgressResolution::WriteZeroBytes
                    | EgressResolution::DeliveryUnknown
            ) {
                return Err(AdapterRuntimeError::EgressIntent);
            }
            let record = self
                .denials
                .get_mut(request_id)
                .filter(|record| {
                    matches!(
                        record.reply,
                        PermissionReply::AllowOnce | PermissionReply::AllowAlways
                    )
                })
                .ok_or(AdapterRuntimeError::EgressIntent)?;
            let previous = record.state;
            record.state = match (resolution, record.state) {
                (EgressResolution::Queued, DenialState::Pending | DenialState::Queued) => {
                    DenialState::Queued
                }
                (EgressResolution::Written, DenialState::Queued | DenialState::Written) => {
                    DenialState::Written
                }
                _ => return Err(AdapterRuntimeError::EgressIntent),
            };
            if record.state == DenialState::Written && previous != DenialState::Written {
                self.active
                    .as_mut()
                    .ok_or(AdapterRuntimeError::EgressIntent)?
                    .translator
                    .permission_allow_written(&record.translator_id)
                    .map_err(map_translation)?;
            }
            return Ok(());
        }
        if let Some(request_id) = token.cancelled_permission_request() {
            if matches!(
                resolution,
                EgressResolution::QueueRejected
                    | EgressResolution::WriteZeroBytes
                    | EgressResolution::DeliveryUnknown
            ) {
                return Err(AdapterRuntimeError::MandatoryDenyTeardown);
            }
            let record = self
                .denials
                .get_mut(request_id)
                .filter(|record| record.reply == PermissionReply::Cancelled)
                .ok_or(AdapterRuntimeError::EgressIntent)?;
            record.state = match (resolution, record.state) {
                (EgressResolution::Queued, DenialState::Committed | DenialState::Queued) => {
                    DenialState::Queued
                }
                (EgressResolution::Written, DenialState::Queued | DenialState::Written) => {
                    DenialState::Written
                }
                _ => return Err(AdapterRuntimeError::EgressIntent),
            };
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
