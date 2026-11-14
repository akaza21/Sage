use crate::acp::bindings::adapter::{AdapterRuntimeError, EgressResolution};

use super::super::super::request::RequestKind;
use super::super::{CodexRuntime, support::map_adapter};

impl CodexRuntime {
    pub(super) fn resolve_request_egress(
        &mut self,
        request_id: u64,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if resolution == EgressResolution::DeliveryUnknown {
            match self.inner.pending_kind(request_id) {
                Some(RequestKind::GoalGet) => self.inner.goal_get_delivery_unknown(request_id),
                Some(RequestKind::GoalPause | RequestKind::GoalClear) => {
                    self.inner.goal_mutation_delivery_unknown(request_id)
                }
                _ => {}
            }
        }
        if matches!(
            resolution,
            EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes
        ) {
            let kind = self.inner.pending_kind(request_id);
            self.inner
                .reject_before_write(request_id)
                .map_err(map_adapter)?;
            if kind.is_some_and(RequestKind::starts_turn) {
                self.snapshots.clear();
            }
            if kind == Some(RequestKind::Models) {
                self.refresh_model_config();
            }
        }
        Ok(())
    }
}
