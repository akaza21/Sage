use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{AdapterBatchJournal, AdapterPumpError},
};

use super::super::{
    DriverError, ProcessTreeControl, StdinTransport, TeardownCause, TeardownDetail, TreeLeaseOwner,
    types::tree_exit,
};
use super::{DriverState, ProcessAdapterDriver};

fn pump_error_summary(error: &AdapterPumpError) -> &'static str {
    match error {
        AdapterPumpError::Intent => "adapter event or command has stale or foreign intent",
        AdapterPumpError::NoActiveTurn => "there is no exact active turn for cancellation",
        AdapterPumpError::TeardownRequired => "adapter runtime tore down fail closed",
        AdapterPumpError::BatchCapacity => "adapter batch exceeded a frozen count or byte limit",
        AdapterPumpError::Backpressure => "adapter egress is backpressured at capacity",
        AdapterPumpError::Journal(_) => "durable adapter journal failed",
        AdapterPumpError::BootstrapRequired => "projection ownership changed",
        AdapterPumpError::RecoveryRequired => "projection delivery failed",
        AdapterPumpError::Frame => "harness sent a malformed protocol frame",
        AdapterPumpError::Refused(_) => "harness refused the request",
        AdapterPumpError::Adapter(_) => "harness adapter rejected the exchange",
    }
}

impl<A, J, R, T, C, L> ProcessAdapterDriver<A, J, R, T, C, L>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    L: TreeLeaseOwner,
{
    pub(crate) fn teardown_cause(&self) -> Option<TeardownCause> {
        match self.state {
            DriverState::Running => None,
            DriverState::TeardownRequired(ref cause) => Some(cause.clone()),
        }
    }

    pub(crate) fn terminal_outcome(
        &self,
    ) -> Option<sage_acp_protocol::lifecycle::TurnTerminalOutcome> {
        let cause = self.teardown_cause()?;
        let reason = sage_acp_protocol::lifecycle::TurnTerminalReason::from(cause.clone());
        let message = match (&cause, self.teardown_detail) {
            (TeardownCause::RefusedVersion { observed }, _) => {
                crate::acp::runtime::terminal_detail::refused_version(observed.as_str(), &[])
            }
            (_, Some(TeardownDetail::Exit(exit))) => {
                crate::acp::runtime::terminal_detail::harness_exited(Some(exit))
            }
            (_, Some(TeardownDetail::Summary(summary))) => {
                crate::acp::runtime::terminal_detail::protocol_error(summary)
            }
            (_, None) => None,
        };
        Some(sage_acp_protocol::lifecycle::TurnTerminalOutcome {
            reason,
            message,
            remediation: None,
        })
    }

    pub(crate) fn teardown_exit(&self) -> Option<crate::acp::process::TreeExit> {
        match (self.teardown_cause(), self.teardown_detail) {
            (Some(_), Some(TeardownDetail::Exit(exit))) => Some(exit),
            (Some(cause), _) => Some(tree_exit(cause)),
            (None, _) => None,
        }
    }

    pub(super) fn ensure_running(&self) -> Result<(), DriverError> {
        match self.state {
            DriverState::Running => Ok(()),
            DriverState::TeardownRequired(ref cause) => {
                Err(DriverError::TeardownRequired(cause.clone()))
            }
        }
    }

    pub(super) fn pump_error(&mut self, error: AdapterPumpError) -> DriverError {
        if matches!(
            error,
            AdapterPumpError::Journal(crate::acp::runtime::PumpJournalError::CommitUnknown)
        ) {
            self.durable_mutation_unknown = true;
        }
        if self.pending_exit.is_some() {
            return self
                .latch_pending_exit()
                .unwrap_or_else(|| self.latch(TeardownCause::ProcessExit));
        }
        if let AdapterPumpError::Refused(refusal) = error {
            return DriverError::Refused(refusal);
        }
        if let AdapterPumpError::Adapter(adapter_error) = &error
            && let Some(nonfatal_refusal) =
                crate::acp::runtime::NonfatalRefusal::from_adapter(adapter_error)
        {
            return DriverError::Refused(nonfatal_refusal);
        }
        let summary = pump_error_summary(&error);
        let cause = match error {
            AdapterPumpError::Frame => TeardownCause::ProtocolFailure,
            AdapterPumpError::Adapter(
                crate::acp::bindings::adapter::AdapterRuntimeError::MandatoryDenyTeardown,
            ) => TeardownCause::MandatoryDenyFailure,
            _ => TeardownCause::PumpFailure,
        };
        self.latch_detailed(cause, Some(TeardownDetail::Summary(summary)))
    }

    pub(super) fn latch(&mut self, cause: TeardownCause) -> DriverError {
        self.latch_detailed(cause, None)
    }

    pub(super) fn latch_detailed(
        &mut self,
        cause: TeardownCause,
        detail: Option<TeardownDetail>,
    ) -> DriverError {
        if matches!(self.state, DriverState::Running) {
            self.transport.close();
            if self
                .pending_exit
                .as_ref()
                .is_none_or(|pending| pending.exit.is_none())
            {
                let _ = self.control.request_termination(tree_exit(cause.clone()));
            }
            self.state = DriverState::TeardownRequired(cause.clone());
            self.teardown_detail = detail;
        }
        DriverError::TeardownRequired(self.teardown_cause().unwrap_or(cause))
    }
}
