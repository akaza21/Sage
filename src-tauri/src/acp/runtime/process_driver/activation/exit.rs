use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::TreeExit,
    runtime::ActivationJournal,
};
use sage_core::limits::LIMITS;

use super::{ActivationDriverError, ActivationProcessDriver, State};
use crate::acp::runtime::process_driver::{
    PendingExit, ProcessTreeControl, StdinTransport, TeardownCause,
};

impl<A, J, R, T, C> ActivationProcessDriver<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: ActivationJournal,
    R: ResourceMapper,
    T: StdinTransport + crate::acp::runtime::ActivationTransport,
    C: ProcessTreeControl,
{
    pub(super) fn record_exit(&mut self, exit: TreeExit, now_millis: i64) {
        if let Some(pending) = self.pending_exit.as_mut() {
            if pending.exit.is_some() {
                return;
            }
            self.transport.close();
            pending.exit = Some(exit);
            pending.deadline_millis = now_millis;
            pending.termination_requested = self.control.request_termination(exit).is_ok();
            return;
        }
        self.transport.close();
        self.pending_exit = Some(PendingExit {
            exit: Some(exit),
            deadline_millis: exit_deadline(now_millis),
            termination_requested: self.control.request_termination(exit).is_ok(),
        });
    }

    pub(super) fn record_reader_eof(&mut self, now_millis: i64) {
        self.pending_exit = Some(PendingExit {
            exit: None,
            deadline_millis: exit_deadline(now_millis),
            termination_requested: false,
        });
    }

    pub(super) fn pending_exit_deadline_millis(&self) -> Option<i64> {
        self.pending_exit.map(|pending| pending.deadline_millis)
    }

    pub(super) fn settle_due_exit(&mut self, now_millis: i64) -> Result<(), ActivationDriverError> {
        if self
            .pending_exit
            .is_some_and(|pending| now_millis >= pending.deadline_millis)
        {
            return Err(self
                .latch_pending_exit()
                .unwrap_or_else(|| self.latch(TeardownCause::ProcessExit)));
        }
        Ok(())
    }

    pub(super) fn latch_pending_exit(&mut self) -> Option<ActivationDriverError> {
        let pending = self.pending_exit?;
        let Some(exit) = pending.exit else {
            self.pending_exit = None;
            return Some(self.latch(TeardownCause::ReaderEof));
        };
        self.pending_exit = None;
        if matches!(self.state, State::Running) {
            self.pump.force_teardown();
            self.transport.close();
            self.state = State::Teardown(TeardownCause::ProcessExit);
        }
        if pending.termination_requested {
            return Some(ActivationDriverError::TeardownRequired(
                TeardownCause::ProcessExit,
            ));
        }
        self.control
            .request_termination(exit)
            .map_or(Some(ActivationDriverError::Tree), |_| {
                Some(ActivationDriverError::TeardownRequired(
                    TeardownCause::ProcessExit,
                ))
            })
    }
}

fn exit_deadline(now_millis: i64) -> i64 {
    i64::try_from(LIMITS.deadlines_ms.tree_termination)
        .map_or(i64::MAX, |budget| now_millis.saturating_add(budget))
}
