//! Delivery and rejected-response handling retain correlation tombstones for
//! all three title tries, without retaining their payloads or process state.
use super::{CodexRuntime, Instant, Step};
use crate::acp::bindings::adapter::EgressResolution;

impl CodexRuntime {
    pub(in super::super) fn may_receive_title_response(&self) -> bool {
        self.title
            .as_ref()
            .is_some_and(|request| !request.ids.is_empty())
    }

    pub(in super::super) fn reject_title_response(&mut self, id: u64) -> bool {
        let Some(request) = self
            .title
            .as_mut()
            .filter(|request| request.ids.contains(&id))
        else {
            return false;
        };
        if request.finished {
            return true;
        }
        let Some(step) = request.settle_reply(id) else {
            return true;
        };
        request.result = Some(None);
        // A malformed allocation or cleanup reply cannot prove remote unload.
        if matches!(step, Step::Start | Step::Unsubscribe) {
            request.cancelled = true;
            request.cleanup_deadline = Some(Instant::now());
        } else {
            request.cancel();
        }
        true
    }

    pub(in super::super) fn title_write(&mut self, id: u64, resolution: EgressResolution) -> bool {
        let Some(request) = &mut self.title else {
            return false;
        };
        if !request.ids.contains(&id) {
            return false;
        }
        if !request.pending.iter().any(|(pending, _)| *pending == id) {
            return true;
        }
        use EgressResolution::*;
        if matches!(resolution, QueueRejected | WriteZeroBytes | DeliveryUnknown) {
            request.result = Some(None);
            if resolution == DeliveryUnknown {
                request.cancelled = true;
                self.poisoned = true;
                request.cleanup_deadline = Some(Instant::now());
            } else {
                request.pending.retain(|(pending, _)| *pending != id);
                request.cancel();
            }
        }
        true
    }
}
