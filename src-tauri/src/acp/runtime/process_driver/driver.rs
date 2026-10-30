use std::sync::Arc;

mod exit;
mod ingress;
mod teardown;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    contract::RuntimeEpoch,
    runtime::{AdapterBatchJournal, AdapterPump, AdapterPumpError, FlushDisposition, HostError},
    scheduler::RuntimeOwner,
};

use super::worker_command::{WorkerCommand, WorkerReply, WorkerResponder};
use super::{
    CancelIntent, DispatchSource, DriverClock, DriverError, DriverProgress, FirstDeliveryUnknown,
    PendingExit, ProcessTreeControl, StdinTransport, TeardownCause, TreeLeaseOwner, WorkerEnvelope,
    types::tree_exit,
};

use super::TeardownDetail;

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub(super) durable_mutation_unknown: bool,
    pub(super) teardown_detail: Option<TeardownDetail>,
    pub(super) pending_exit: Option<PendingExit>,
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
#[allow(
    dead_code,
    reason = "the app acts on the worker envelope; only tests read the progress"
)]
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
    #[cfg(test)]
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
            durable_mutation_unknown: false,
            teardown_detail: None,
            pending_exit: None,
        }
    }

    pub(crate) fn latest_durable_turn_cursor(
        &self,
    ) -> Option<&crate::acp::store::AdapterTurnCursor> {
        self.pump.latest_durable_turn_cursor()
    }

    pub(crate) fn close_stdin(&mut self) {
        self.transport.close();
    }

    pub(crate) fn runtime_owner(&self) -> &RuntimeOwner {
        &self.owner
    }

    pub(crate) fn first_delivery_unknown(&self) -> Option<FirstDeliveryUnknown> {
        self.first_delivery_unknown
    }

    pub(crate) fn take_durable_mutation_unknown(&mut self) -> bool {
        std::mem::take(&mut self.durable_mutation_unknown)
    }

    pub(crate) fn dispatch_prompt(
        &mut self,
        prompt: crate::acp::intent::Prompt,
        client_message_id: crate::acp::contract::RequiredText<512>,
    ) -> Result<DriverProgress, DriverError> {
        self.dispatch_prompt_with_access_mode(
            prompt,
            client_message_id,
            crate::settings_store::SessionAccessMode::ReadOnly,
        )
    }

    pub(crate) fn dispatch_prompt_with_access_mode(
        &mut self,
        prompt: crate::acp::intent::Prompt,
        client_message_id: crate::acp::contract::RequiredText<512>,
        access_mode: crate::settings_store::SessionAccessMode,
    ) -> Result<DriverProgress, DriverError> {
        self.ensure_running()?;
        if let Some(error) = self.latch_pending_exit() {
            return Err(error);
        }
        match self
            .pump
            .queue_prompt(prompt, &client_message_id, access_mode)
        {
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

    pub(crate) async fn dispatch_cancel(
        &mut self,
        intent: CancelIntent,
    ) -> Result<DriverProgress, DriverError> {
        self.ensure_running()?;
        if self.pending_exit.is_some() {
            return Err(self.settle_exit(self.clock.now_millis()).await);
        }
        self.cancel(intent, DispatchSource::Cancel).await
    }

    pub(crate) fn dispatch_steer(
        &mut self,
        prompt: crate::acp::intent::Prompt,
        client_message_id: crate::acp::contract::RequiredText<512>,
    ) -> Result<(DriverProgress, u64), DriverError> {
        self.ensure_running()?;
        if let Some(error) = self.latch_pending_exit() {
            return Err(error);
        }
        let request_id = match self.pump.queue_steer(prompt, client_message_id) {
            Ok((_, request_id)) => request_id,
            Err(AdapterPumpError::NoActiveTurn | AdapterPumpError::Intent) => {
                return Err(DriverError::RejectedIntent);
            }
            Err(error) => return Err(self.pump_error(error)),
        };
        let mut progress = DriverProgress::default();
        self.flush_all(DispatchSource::Prompt, &mut progress)?;
        Ok((progress, request_id))
    }

    pub(crate) fn take_steer_response(&mut self, request_id: u64) -> Option<bool> {
        self.pump.take_steer_response(request_id)
    }

    pub(crate) fn expire_steer(&mut self, request_id: u64) {
        self.pump.expire_steer(request_id);
    }

    pub(crate) fn dispatch_close_session(&mut self) -> Result<DriverProgress, DriverError> {
        self.ensure_running()?;
        if let Some(error) = self.latch_pending_exit() {
            return Err(error);
        }
        match self
            .pump
            .queue_command(crate::acp::bindings::adapter::AdapterCommand::CloseSession)
        {
            Ok(_) => {}
            Err(AdapterPumpError::NoActiveTurn | AdapterPumpError::Intent) => {
                return Err(DriverError::RejectedIntent);
            }
            Err(error) => return Err(self.pump_error(error)),
        }
        let mut progress = DriverProgress::default();
        self.flush_all(DispatchSource::Cancel, &mut progress)?;
        Ok(progress)
    }

    pub(crate) async fn resolve_permission(
        &mut self,
        request_id: &crate::acp::contract::SageServerRequestId,
        decision: crate::acp::runtime::permissions::PermissionDecision,
    ) -> Result<(), DriverError> {
        self.ensure_running()?;
        if self.pending_exit.is_some() {
            return Err(self.settle_exit(self.clock.now_millis()).await);
        }
        self.pump
            .resolve_permission(request_id, decision, self.clock.now_millis())
            .await
            .map_err(|error| self.pump_error(error))?;
        let mut progress = DriverProgress::default();
        self.flush_all_ingress(
            DispatchSource::Ingress,
            self.clock.now_millis(),
            &mut progress,
        )
        .await?;
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
            DriverState::TeardownRequired(ref cause) => {
                Err(DriverError::TeardownRequired(cause.clone()))
            }
        }
    }

    async fn cancel(
        &mut self,
        intent: CancelIntent,
        source: DispatchSource,
    ) -> Result<DriverProgress, DriverError> {
        if !intent.matches_owner(&self.owner) {
            return Err(DriverError::IdentityMismatch);
        }
        let cancelled_permission_frames = match self
            .pump
            .queue_cancelled_permissions(
                &intent.session_id,
                intent.runtime_epoch,
                &intent.turn_id,
                &intent.client_message_id,
                self.clock.now_millis(),
            )
            .await
        {
            Ok(count) => count,
            Err(AdapterPumpError::NoActiveTurn | AdapterPumpError::Intent) => {
                return Err(DriverError::RejectedIntent);
            }
            Err(error) => return Err(self.pump_error(error)),
        };
        let mut progress = DriverProgress {
            cancelled_permission_frames,
            ..DriverProgress::default()
        };
        self.flush_all(source, &mut progress)?;
        match self.pump.queue_cancel_notification(
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
                    return Err(self
                        .latch_pending_exit()
                        .unwrap_or_else(|| self.latch(TeardownCause::DeliveryUnknown)));
                }
                Err(error) => return Err(self.pump_error(error)),
            }
        }
    }

    pub(super) async fn flush_all_ingress(
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
                    return Err(self
                        .latch_pending_exit()
                        .unwrap_or_else(|| self.latch(TeardownCause::DeliveryUnknown)));
                }
                Err(error) => return Err(self.pump_error(error)),
            }
        }
    }
}
