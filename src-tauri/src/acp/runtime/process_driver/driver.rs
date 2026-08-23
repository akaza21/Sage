use std::sync::Arc;

mod ingress;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    contract::RuntimeEpoch,
    runtime::{AdapterBatchJournal, AdapterPump, AdapterPumpError, FlushDisposition, HostError},
    scheduler::RuntimeOwner,
};

use super::worker_command::{WorkerCommand, WorkerReply, WorkerResponder};
use super::{
    CancelIntent, DispatchSource, DriverClock, DriverError, DriverProgress, FirstDeliveryUnknown,
    ProcessTreeControl, StdinTransport, TeardownCause, TreeLeaseOwner, WorkerEnvelope,
    types::tree_exit,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DriverState {
    Running,
    TeardownRequired(TeardownCause),
}

pub(crate) struct ProcessAdapterDriver<A, J, R, T, C, L>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    L: TreeLeaseOwner,
{
    pub(super) pump: AdapterPump<A, J, R>,
    pub(super) transport: T,
    pub(super) control: C,
    pub(super) lease: Option<L>,
    pub(super) owner: RuntimeOwner,
    pub(super) clock: Arc<dyn DriverClock>,
    pub(super) state: DriverState,
    pub(super) first_delivery_unknown: Option<FirstDeliveryUnknown>,
}

pub(crate) struct DriverAdmission;

pub(super) fn monotonic_deadline_millis(after: std::time::Duration) -> u64 {
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    ORIGIN
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
        .saturating_add(after.as_millis())
        .min(u128::from(u64::MAX)) as u64
}

impl WorkerEnvelope {
    pub(crate) fn respond(self, reply: Result<WorkerReply, HostError>) {
        let _ = self.reply.send(reply);
    }

    pub(crate) fn reject(self, error: HostError) {
        self.respond(Err(error));
    }

    pub(crate) fn into_parts(self) -> (WorkerCommand, WorkerResponder) {
        (self.command, WorkerResponder(self.reply))
    }

    pub(crate) fn deadline(&self) -> std::time::Instant {
        self.deadline
    }
}

impl WorkerResponder {
    pub(crate) fn respond(self, reply: Result<WorkerReply, HostError>) {
        let _ = self.0.send(reply);
    }
}

// Worker envelopes are bounded by the single reserved mailbox; keeping them
// inline avoids a heap allocation on every control-lane handoff.
#[allow(clippy::large_enum_variant)]
pub(crate) enum DriverStep {
    Progress(DriverProgress),
    YieldWorker {
        progress: DriverProgress,
        envelope: WorkerEnvelope,
    },
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
    pub(crate) fn new(
        pump: AdapterPump<A, J, R>,
        transport: T,
        control: C,
        lease: L,
        owner: RuntimeOwner,
        clock: Arc<dyn DriverClock>,
    ) -> Result<Self, DriverError> {
        let admission = Self::admit(&lease, &owner)?;
        Ok(Self::from_admission(
            pump, transport, control, lease, owner, clock, admission,
        ))
    }

    pub(crate) fn admit(lease: &L, owner: &RuntimeOwner) -> Result<DriverAdmission, DriverError> {
        if lease.runtime_owner() != owner || owner.runtime_epoch == RuntimeEpoch::ZERO {
            return Err(DriverError::IdentityMismatch);
        }
        Ok(DriverAdmission)
    }

    pub(crate) fn from_admission(
        pump: AdapterPump<A, J, R>,
        transport: T,
        control: C,
        lease: L,
        owner: RuntimeOwner,
        clock: Arc<dyn DriverClock>,
        _admission: DriverAdmission,
    ) -> Self {
        Self {
            pump,
            transport,
            control,
            lease: Some(lease),
            owner,
            clock,
            state: DriverState::Running,
            first_delivery_unknown: None,
        }
    }

    pub(crate) fn latest_durable_turn_cursor(
        &self,
    ) -> Option<&crate::acp::store::AdapterTurnCursor> {
        self.pump.latest_durable_turn_cursor()
    }

    pub(crate) fn runtime_owner(&self) -> &RuntimeOwner {
        &self.owner
    }

    pub(crate) fn first_delivery_unknown(&self) -> Option<FirstDeliveryUnknown> {
        self.first_delivery_unknown
    }

    pub(crate) fn teardown_cause(&self) -> Option<TeardownCause> {
        match self.state {
            DriverState::Running => None,
            DriverState::TeardownRequired(cause) => Some(cause),
        }
    }

    pub(crate) fn dispatch_prompt(
        &mut self,
        prompt: crate::acp::intent::Prompt,
        client_message_id: crate::acp::contract::RequiredText<512>,
    ) -> Result<DriverProgress, DriverError> {
        self.ensure_running()?;
        match self.pump.queue_prompt(prompt, &client_message_id) {
            Ok(_) => {}
            Err(AdapterPumpError::NoActiveTurn | AdapterPumpError::Intent) => {
                return Err(DriverError::RejectedIntent);
            }
            Err(error) => return Err(self.pump_error(error)),
        }
        let mut progress = DriverProgress::default();
        self.flush_all(DispatchSource::Prompt, &mut progress)?;
        Ok(progress)
    }

    pub(crate) fn dispatch_cancel(
        &mut self,
        intent: CancelIntent,
    ) -> Result<DriverProgress, DriverError> {
        self.ensure_running()?;
        self.cancel(intent, DispatchSource::Cancel)
    }

    pub(crate) async fn resolve_permission(
        &mut self,
        request_id: &crate::acp::contract::SageServerRequestId,
        decision: crate::acp::runtime::permissions::PermissionDecision,
    ) -> Result<(), DriverError> {
        self.ensure_running()?;
        self.pump
            .resolve_permission(request_id, decision, self.clock.now_millis())
            .await
            .map_err(|error| self.pump_error(error))?;
        let mut progress = DriverProgress::default();
        self.flush_all(DispatchSource::Ingress, &mut progress)?;
        if progress.zero_byte_failures != 0 {
            return Err(self.latch(TeardownCause::MandatoryDenyFailure));
        }
        Ok(())
    }

    pub(crate) fn request_host_termination(&mut self) -> Result<(), DriverError> {
        match self.state {
            DriverState::Running => {
                self.transport.close();
                self.control
                    .request_termination(tree_exit(TeardownCause::HostTermination))
                    .map_err(|_| DriverError::Tree)?;
                self.state = DriverState::TeardownRequired(TeardownCause::HostTermination);
                Ok(())
            }
            DriverState::TeardownRequired(TeardownCause::HostTermination) => Ok(()),
            DriverState::TeardownRequired(cause) => Err(DriverError::TeardownRequired(cause)),
        }
    }

    fn cancel(
        &mut self,
        intent: CancelIntent,
        source: DispatchSource,
    ) -> Result<DriverProgress, DriverError> {
        if !intent.matches_owner(&self.owner) {
            return Err(DriverError::IdentityMismatch);
        }
        match self.pump.queue_cancel(
            &intent.session_id,
            intent.runtime_epoch,
            &intent.turn_id,
            &intent.client_message_id,
        ) {
            Ok(_) => {}
            Err(AdapterPumpError::NoActiveTurn | AdapterPumpError::Intent) => {
                return Err(DriverError::RejectedIntent);
            }
            Err(error) => return Err(self.pump_error(error)),
        }
        let mut progress = DriverProgress::default();
        self.flush_all(source, &mut progress)?;
        Ok(progress)
    }

    fn flush_all(
        &mut self,
        source: DispatchSource,
        progress: &mut DriverProgress,
    ) -> Result<(), DriverError> {
        loop {
            match self.pump.flush_one(&mut self.transport) {
                Ok(FlushDisposition::Idle) => return Ok(()),
                Ok(FlushDisposition::Written) => progress.written_frames += 1,
                Ok(FlushDisposition::ZeroBytes) => progress.zero_byte_failures += 1,
                Ok(FlushDisposition::DeliveryUnknown) => {
                    self.first_delivery_unknown
                        .get_or_insert(FirstDeliveryUnknown { source });
                    return Err(self.latch(TeardownCause::DeliveryUnknown));
                }
                Err(error) => return Err(self.pump_error(error)),
            }
        }
    }

    async fn flush_all_ingress(
        &mut self,
        source: DispatchSource,
        now_millis: i64,
        progress: &mut DriverProgress,
    ) -> Result<(), DriverError> {
        loop {
            match self.pump.flush_one(&mut self.transport) {
                Ok(FlushDisposition::Idle) => return Ok(()),
                Ok(FlushDisposition::Written) => {
                    progress.written_frames += 1;
                    if let Err(error) = self.pump.settle_written_permission(now_millis).await {
                        return Err(self.pump_error(error));
                    }
                }
                Ok(FlushDisposition::ZeroBytes) => progress.zero_byte_failures += 1,
                Ok(FlushDisposition::DeliveryUnknown) => {
                    self.first_delivery_unknown
                        .get_or_insert(FirstDeliveryUnknown { source });
                    return Err(self.latch(TeardownCause::DeliveryUnknown));
                }
                Err(error) => return Err(self.pump_error(error)),
            }
        }
    }

    fn pump_error(&mut self, error: AdapterPumpError) -> DriverError {
        let cause = match error {
            AdapterPumpError::Frame => TeardownCause::ProtocolFailure,
            AdapterPumpError::Adapter(
                crate::acp::bindings::adapter::AdapterRuntimeError::MandatoryDenyTeardown,
            ) => TeardownCause::MandatoryDenyFailure,
            _ => TeardownCause::PumpFailure,
        };
        self.latch(cause)
    }

    fn ensure_running(&self) -> Result<(), DriverError> {
        match self.state {
            DriverState::Running => Ok(()),
            DriverState::TeardownRequired(cause) => Err(DriverError::TeardownRequired(cause)),
        }
    }

    fn latch(&mut self, cause: TeardownCause) -> DriverError {
        if matches!(self.state, DriverState::Running) {
            self.transport.close();
            let _ = self.control.request_termination(tree_exit(cause));
            self.state = DriverState::TeardownRequired(cause);
        }
        DriverError::TeardownRequired(self.teardown_cause().unwrap_or(cause))
    }
}
