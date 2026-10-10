use crate::acp::{
    bindings::adapter::{
        AdapterEgress, AdapterEvent, AdapterRuntimeError, EgressResolution, EgressToken,
    },
    contract::SafeText,
    intent::NormalizedPayload,
    lifecycle::ItemLifecycle,
    limits::LIMITS,
    transport::OutboundFrame,
};

use super::super::{DenyStage, PiRuntime};

const DIALOG_NOTICES_PER_TURN: u8 = 8;

impl PiRuntime {
    pub(super) fn cancel_dialog(
        &mut self,
        id: String,
    ) -> Result<(Option<AdapterEvent>, Option<AdapterEgress>), AdapterRuntimeError> {
        if self.cancelled_dialogs.contains_key(&id) || self.recent_cancelled_dialogs.contains(&id) {
            return Ok((Some(AdapterEvent::IgnoredDuplicate), None));
        }
        if self.cancelled_dialogs.len() >= LIMITS.queues.outstanding_rpc_per_session as usize {
            self.poison();
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let external = super::identity::external("piDialogId", id.clone())?;
        let bytes = serde_json::to_vec(&serde_json::json!({
            "type":"extension_ui_response", "id":id, "cancelled":true,
        }))
        .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
        let frame =
            OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
        self.cancelled_dialogs.insert(id, DenyStage::Prepared);
        let event = match self.active_turn.as_mut() {
            None => AdapterEvent::IgnoredLate,
            Some(turn) if turn.dialog_notices >= DIALOG_NOTICES_PER_TURN => AdapterEvent::Absorbed,
            Some(turn) => {
                turn.dialog_notices += 1;
                let payload = NormalizedPayload::Unsupported {
                    external_kind: SafeText::new("externalKind", "pi_dialog_cancelled")
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                    summary: SafeText::new(
                        "summary",
                        "Pi extension dialog was cancelled: this dialog is not supported",
                    )
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                };
                let item_id = self.next_local_item("dialog")?;
                self.item(item_id, ItemLifecycle::Cancelled, None, payload)?
            }
        };
        Ok((
            Some(event),
            Some(AdapterEgress::new(
                frame,
                EgressToken::permission_cancel(external),
            )),
        ))
    }

    pub(in crate::acp::bindings::pi::runtime) fn commit_dialog_cancel(
        &mut self,
        token: &EgressToken,
    ) -> Result<(), AdapterRuntimeError> {
        let id = token
            .cancelled_permission_request()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        let stage = self
            .cancelled_dialogs
            .get_mut(id)
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if *stage == DenyStage::Prepared {
            *stage = DenyStage::Committed;
        }
        Ok(())
    }

    pub(in crate::acp::bindings::pi::runtime) fn resolve_dialog_cancel(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        let id = token
            .cancelled_permission_request()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if matches!(
            resolution,
            EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown
        ) {
            self.poison();
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let stage = self
            .cancelled_dialogs
            .get_mut(id)
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        match (*stage, resolution) {
            (DenyStage::Committed, EgressResolution::Queued) => *stage = DenyStage::Queued,
            (DenyStage::Queued, EgressResolution::Written) => {
                if self.recent_cancelled_dialogs.len()
                    == LIMITS.queues.outstanding_rpc_per_session as usize
                {
                    self.recent_cancelled_dialogs.pop_front();
                }
                self.recent_cancelled_dialogs.push_back(id.to_owned());
                self.cancelled_dialogs.remove(id);
            }
            (DenyStage::Queued, EgressResolution::Queued) => {}
            _ => return Err(AdapterRuntimeError::EgressIntent),
        }
        Ok(())
    }
}
