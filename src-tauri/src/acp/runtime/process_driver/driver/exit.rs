use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::TreeExit,
    runtime::AdapterBatchJournal,
};
use sage_core::limits::LIMITS;

use super::ProcessAdapterDriver;
use crate::acp::runtime::process_driver::{
    DriverError, PendingExit, ProcessTreeControl, StdinTransport, TeardownCause, TeardownDetail,
    TreeLeaseOwner,
};

impl<A, J, R, T, C, L> ProcessAdapterDriver<A, J, R, T, C, L>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    L: TreeLeaseOwner,
{
    pub(super) fn record_exit(&mut self, exit: TreeExit, now_millis: i64) {
        if let Some(pending) = self.pending_exit.as_mut() {
            if pending.exit.is_some() {
                return;
            }
            pending.exit = Some(exit);
            pending.deadline_millis = now_millis;
            self.transport.close();
            pending.termination_requested = self.control.request_termination(exit).is_ok();
            return;
        }
        let deadline_millis = i64::try_from(LIMITS.deadlines_ms.tree_termination)
            .map_or(i64::MAX, |budget| now_millis.saturating_add(budget));
        self.transport.close();
        let termination_requested = self.control.request_termination(exit).is_ok();
        self.pending_exit = Some(PendingExit {
            exit: Some(exit),
            deadline_millis,
            termination_requested,
        });
    }

    pub(super) fn record_reader_eof(&mut self, now_millis: i64) {
        if self.pending_exit.is_some() {
            return;
        }
        let deadline_millis = i64::try_from(LIMITS.deadlines_ms.tree_termination)
            .map_or(i64::MAX, |budget| now_millis.saturating_add(budget));
        self.pending_exit = Some(PendingExit {
            exit: None,
            deadline_millis,
            termination_requested: false,
        });
    }

    pub(crate) fn pending_exit_deadline_millis(&self) -> Option<i64> {
        self.pending_exit.map(|pending| pending.deadline_millis)
    }

    pub(super) async fn settle_due_exit(&mut self, now_millis: i64) -> Result<(), DriverError> {
        let Some(pending) = self.pending_exit else {
            return Ok(());
        };
        if now_millis < pending.deadline_millis {
            return Ok(());
        }
        Err(self.settle_exit(now_millis).await)
    }

    pub(super) async fn settle_exit(&mut self, now_millis: i64) -> DriverError {
        let error = match self.latch_pending_exit() {
            Some(error) => error,
            None => self.latch(TeardownCause::ReaderEof),
        };
        let _ = self.finish_stdout(now_millis).await;
        error
    }

    pub(super) fn latch_pending_exit(&mut self) -> Option<DriverError> {
        let pending = self.pending_exit?;
        let Some(exit) = pending.exit else {
            let error = self.latch(TeardownCause::ReaderEof);
            self.pending_exit = None;
            return Some(error);
        };
        let error =
            self.latch_detailed(TeardownCause::ProcessExit, Some(TeardownDetail::Exit(exit)));
        self.pending_exit = None;
        if pending.termination_requested {
            return Some(error);
        }
        match self.control.request_termination(exit) {
            Ok(()) => Some(error),
            Err(_) => Some(DriverError::Tree),
        }
    }

    pub(super) async fn latch_after_reader_terminal(
        &mut self,
        cause: TeardownCause,
        now_millis: i64,
    ) -> DriverError {
        if self.pending_exit.is_some() {
            return self.settle_exit(now_millis).await;
        }
        self.latch(cause)
    }
}
