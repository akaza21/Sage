use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{AdapterBatchJournal, DriverClock, HostError, SteerCompletionSink},
};

use super::{PreparedProcessWrite, ProcessTreeControl, StdinTransport};

pub(super) struct PendingSteer {
    request_id: u64,
    completion: SteerCompletionSink,
    deadline: std::time::Instant,
}

impl<A, J, R, T, C> PreparedProcessWrite<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) fn begin_steer(
        &mut self,
        request_id: u64,
        deadline: std::time::Instant,
        completion: SteerCompletionSink,
    ) {
        self.pending_steer = Some(PendingSteer {
            request_id,
            completion,
            deadline,
        });
    }

    pub(crate) fn steer_pending(&self) -> bool {
        self.pending_steer.is_some()
    }

    pub(crate) fn pending_steer_deadline_millis(&self, clock: &dyn DriverClock) -> Option<i64> {
        self.pending_steer.as_ref().map(|pending| {
            let remaining = pending
                .deadline
                .saturating_duration_since(std::time::Instant::now());
            clock
                .now_millis()
                .saturating_add(i64::try_from(remaining.as_millis()).unwrap_or(i64::MAX))
        })
    }

    pub(crate) fn settle_steer(&mut self) {
        let Some(pending) = self.pending_steer.as_ref() else {
            return;
        };
        let response = self.turn.driver.take_steer_response(pending.request_id);
        let result = if std::time::Instant::now() >= pending.deadline {
            Some(Err(HostError::SteerOutcomeUnknown))
        } else if self.has_durable_terminal() {
            Some(Err(HostError::Intent))
        } else {
            response.map(|accepted| {
                if accepted {
                    Ok(())
                } else {
                    Err(HostError::Intent)
                }
            })
        };
        if let Some(result) = result {
            let pending = self.pending_steer.take().expect("pending Steer checked");
            self.turn.driver.expire_steer(pending.request_id);
            pending.completion.complete(result);
        }
    }

    pub(crate) fn reject_steer(&mut self, error: HostError) {
        if let Some(pending) = self.pending_steer.take() {
            self.turn.driver.expire_steer(pending.request_id);
            pending.completion.complete(Err(error));
        }
    }
}
