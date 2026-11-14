use std::sync::Arc;

mod diagnostic;
mod exit;
mod handoff;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{
        ActivationFlush, ActivationJournal, ActivationPump, ActivationPumpError,
        ActivationStartupStep,
    },
    scheduler::RuntimeOwner,
};
use diagnostic::safe_frame_diagnostic;

use super::types::tree_exit;
use super::{
    ControlMessage, DispatchSource, DriverClock, DriverMailbox, DriverProgress,
    FirstDeliveryUnknown, PendingExit, ProcessTreeControl, ReaderTerminal, StdinTransport,
    TeardownCause,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Running,
    ProjectionRecovery,
    Teardown(TeardownCause),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ActivationDriverError {
    #[error("the activation owner or runtime epoch is stale")]
    IdentityMismatch,
    #[error("the activation mailbox rejected work")]
    Mailbox,
    #[error("the activation requires authoritative projection recovery")]
    ProjectionRecoveryRequired,
    #[error("the activation runtime requires teardown")]
    TeardownRequired(TeardownCause),
    #[error("activation ownership is not quiescent")]
    NotReusable,
    #[error("zero-process convergence is not yet proven")]
    Tree,
}

pub(crate) struct ActivationProcessDriver<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: ActivationJournal,
    R: ResourceMapper,
    T: StdinTransport + crate::acp::runtime::ActivationTransport,
    C: ProcessTreeControl,
{
    pump: ActivationPump<A, J, R>,
    transport: T,
    control: C,
    owner: RuntimeOwner,
    clock: Arc<dyn DriverClock>,
    state: State,
    first_delivery_unknown: Option<FirstDeliveryUnknown>,
    pending_exit: Option<PendingExit>,
}

pub(crate) type RetainedActivationDriverConstruction<A, J, R, T, C> = Result<
    ActivationProcessDriver<A, J, R, T, C>,
    Box<(
        ActivationPump<A, J, R>,
        T,
        C,
        RuntimeOwner,
        Arc<dyn DriverClock>,
        ActivationDriverError,
    )>,
>;

impl<A, J, R, T, C> ActivationProcessDriver<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: ActivationJournal,
    R: ResourceMapper,
    T: StdinTransport + crate::acp::runtime::ActivationTransport,
    C: ProcessTreeControl,
{
    #[cfg(any(test, all(windows, feature = "acp-fixture")))]
    pub(crate) fn new(
        pump: ActivationPump<A, J, R>,
        transport: T,
        control: C,
        owner: RuntimeOwner,
        clock: Arc<dyn DriverClock>,
    ) -> Result<Self, ActivationDriverError> {
        Self::new_retained(pump, transport, control, owner, clock).map_err(|failure| failure.5)
    }

    pub(crate) fn new_retained(
        pump: ActivationPump<A, J, R>,
        transport: T,
        control: C,
        owner: RuntimeOwner,
        clock: Arc<dyn DriverClock>,
    ) -> RetainedActivationDriverConstruction<A, J, R, T, C> {
        if pump.runtime_owner() != owner
            || owner.runtime_epoch == crate::acp::contract::RuntimeEpoch::ZERO
        {
            return Err(Box::new((
                pump,
                transport,
                control,
                owner,
                clock,
                ActivationDriverError::IdentityMismatch,
            )));
        }
        Ok(Self {
            pump,
            transport,
            control,
            owner,
            clock,
            state: State::Running,
            first_delivery_unknown: None,
            pending_exit: None,
        })
    }

    pub(crate) fn fail_before_runner(&mut self) {
        if matches!(self.state, State::Running) {
            let _ = self
                .latch_pending_exit()
                .unwrap_or_else(|| self.latch(TeardownCause::HostTermination));
        }
    }

    pub(crate) fn begin(&mut self) -> Result<DriverProgress, ActivationDriverError> {
        self.ensure_running()?;
        self.pump.begin().map_err(|error| self.pump_error(error))?;
        let mut progress = DriverProgress::default();
        self.flush_all(&mut progress)?;
        Ok(progress)
    }

    pub(crate) fn startup_step(&self) -> ActivationStartupStep {
        self.pump.startup_step()
    }

    pub(crate) fn log_startup_failure(
        &self,
        checkpoint: &'static str,
        error: &ActivationDriverError,
    ) {
        tracing::warn!(
            activation_checkpoint = checkpoint,
            activation_stage = "activation",
            activation_phase = self.pump.startup_step().as_str(),
            ?error,
            "ACP activation failed closed"
        );
    }

    pub(crate) async fn run_next(
        &mut self,
        mailbox: &DriverMailbox,
    ) -> Result<DriverProgress, ActivationDriverError> {
        self.ensure_running()?;
        self.settle_due_exit(self.clock.now_millis())?;
        let message = mailbox
            .wait_next_until(self.pending_exit_deadline_millis(), self.clock.as_ref())
            .map_err(|_| ActivationDriverError::Mailbox)?
            .ok_or_else(|| {
                self.settle_due_exit(self.clock.now_millis())
                    .err()
                    .unwrap_or(ActivationDriverError::Mailbox)
            })?;
        self.dispatch(message, mailbox, self.clock.now_millis())
            .await
    }

    async fn dispatch(
        &mut self,
        message: ControlMessage,
        mailbox: &DriverMailbox,
        now_millis: i64,
    ) -> Result<DriverProgress, ActivationDriverError> {
        match message {
            ControlMessage::StdoutChunk(chunk) => {
                self.accept_chunk(&chunk, mailbox, now_millis).await
            }
            ControlMessage::Prompt(intent) => {
                intent.reject(super::DriverError::RejectedIntent);
                Ok(DriverProgress {
                    rejected_controls: 1,
                    ..DriverProgress::default()
                })
            }
            ControlMessage::Cancel(_) => {
                let progress = DriverProgress {
                    rejected_controls: 1,
                    ..DriverProgress::default()
                };
                Ok(progress)
            }
            ControlMessage::Worker(envelope) => {
                envelope.reject(crate::acp::runtime::HostError::State);
                Ok(DriverProgress {
                    rejected_controls: 1,
                    ..DriverProgress::default()
                })
            }
            ControlMessage::ReaderTerminal(ReaderTerminal::Eof) => {
                self.finish_stdout(now_millis).await?;
                if let Some(error) = self.latch_pending_exit() {
                    self.log_startup_failure("activation.dispatch.reader_eof", &error);
                    return Err(error);
                }
                self.record_reader_eof(now_millis);
                Ok(DriverProgress::default())
            }
            ControlMessage::ReaderTerminal(ReaderTerminal::Io) => {
                if let Some(error) = self.latch_pending_exit() {
                    self.log_startup_failure("activation.dispatch.reader_io", &error);
                    return Err(error);
                }
                self.log_startup_failure(
                    "activation.dispatch.reader_io",
                    &ActivationDriverError::TeardownRequired(TeardownCause::ReaderIo),
                );
                Err(self.latch(TeardownCause::ReaderIo))
            }
            ControlMessage::ProcessExit(exit) => {
                let reader_eof_pending = self
                    .pending_exit
                    .is_some_and(|pending| pending.exit.is_none());
                self.record_exit(exit, now_millis);
                if reader_eof_pending {
                    return Err(self
                        .latch_pending_exit()
                        .unwrap_or_else(|| self.latch(TeardownCause::ProcessExit)));
                }
                Ok(DriverProgress::default())
            }
        }
    }

    async fn accept_chunk(
        &mut self,
        chunk: &[u8],
        mailbox: &DriverMailbox,
        now_millis: i64,
    ) -> Result<DriverProgress, ActivationDriverError> {
        let mut progress = DriverProgress::default();
        let mut produced = match self.pump.push_stdout_chunk(chunk, now_millis).await {
            Ok(produced) => produced,
            Err(error) => {
                self.log_frame_failure("activation.accept_chunk", chunk, &error);
                return Err(self.pump_error(error));
            }
        };
        loop {
            if produced {
                progress.accepted_frames += 1;
                self.flush_all(&mut progress)?;
            }
            self.dispatch_urgent(mailbox, &mut progress)?;
            produced = match self.pump.drain_stdout(now_millis).await {
                Ok(produced) => produced,
                Err(error) => {
                    let driver_error = self.pump_error(error);
                    self.log_startup_failure("activation.accept_chunk.drain_stdout", &driver_error);
                    return Err(driver_error);
                }
            };
            if !produced {
                return Ok(progress);
            }
        }
    }

    async fn finish_stdout(&mut self, now_millis: i64) -> Result<(), ActivationDriverError> {
        loop {
            let produced = self
                .pump
                .finish_stdout(now_millis)
                .await
                .map_err(|error| self.pump_error(error))?;
            if !produced {
                return Ok(());
            }
            let mut progress = DriverProgress::default();
            self.flush_all(&mut progress)?;
        }
    }

    fn dispatch_urgent(
        &mut self,
        mailbox: &DriverMailbox,
        progress: &mut DriverProgress,
    ) -> Result<(), ActivationDriverError> {
        while let Some(control) = mailbox.try_pop_control() {
            match control {
                ControlMessage::Cancel(_) => progress.rejected_controls += 1,
                ControlMessage::ProcessExit(exit) => {
                    self.record_exit(exit, self.clock.now_millis());
                }
                ControlMessage::ReaderTerminal(_) | ControlMessage::StdoutChunk(_) => {
                    return Err(self.latch(TeardownCause::ProtocolFailure));
                }
                ControlMessage::Prompt(intent) => {
                    intent.reject(super::DriverError::RejectedIntent);
                    progress.rejected_controls += 1;
                }
                ControlMessage::Worker(envelope) => {
                    envelope.reject(crate::acp::runtime::HostError::State);
                    progress.rejected_controls += 1;
                }
            }
        }
        Ok(())
    }

    fn flush_all(&mut self, progress: &mut DriverProgress) -> Result<(), ActivationDriverError> {
        loop {
            match self.pump.flush_one(&mut self.transport) {
                Ok(ActivationFlush::Idle) => return Ok(()),
                Ok(ActivationFlush::Written) => progress.written_frames += 1,
                Ok(ActivationFlush::RetriedZeroBytes) => progress.zero_byte_failures += 1,
                Ok(ActivationFlush::DeliveryUnknown) => {
                    self.first_delivery_unknown = Some(FirstDeliveryUnknown {
                        source: DispatchSource::Activation,
                    });
                    return Err(self
                        .latch_pending_exit()
                        .unwrap_or_else(|| self.latch(TeardownCause::DeliveryUnknown)));
                }
                Err(error) => return Err(self.pump_error(error)),
            }
        }
    }

    fn pump_error(&mut self, error: ActivationPumpError) -> ActivationDriverError {
        if let Some(error) = self.latch_pending_exit() {
            return error;
        }
        if matches!(
            error,
            ActivationPumpError::BootstrapRequired | ActivationPumpError::RecoveryRequired
        ) {
            self.state = State::ProjectionRecovery;
            return ActivationDriverError::ProjectionRecoveryRequired;
        }
        self.latch(match error {
            ActivationPumpError::Frame | ActivationPumpError::Intent => {
                TeardownCause::ProtocolFailure
            }
            ActivationPumpError::RefusedVersion(observed) => {
                TeardownCause::RefusedVersion { observed }
            }
            ActivationPumpError::Adapter(
                crate::acp::bindings::adapter::AdapterRuntimeError::MandatoryDenyTeardown,
            ) => TeardownCause::MandatoryDenyFailure,
            _ => TeardownCause::PumpFailure,
        })
    }

    fn log_frame_failure(
        &self,
        checkpoint: &'static str,
        chunk: &[u8],
        error: &ActivationPumpError,
    ) {
        let (json_rpc_method, frame_json, frame_hex) = safe_frame_diagnostic(chunk);
        tracing::warn!(
            activation_checkpoint = checkpoint,
            activation_stage = "activation",
            activation_phase = self.pump.startup_step().as_str(),
            ?error,
            ?json_rpc_method,
            %frame_json,
            %frame_hex,
            "ACP activation rejected inbound frame"
        );
    }

    fn ensure_running(&self) -> Result<(), ActivationDriverError> {
        match self.state {
            State::Running => Ok(()),
            State::ProjectionRecovery => Err(ActivationDriverError::ProjectionRecoveryRequired),
            State::Teardown(ref cause) => {
                Err(ActivationDriverError::TeardownRequired(cause.clone()))
            }
        }
    }

    fn latch(&mut self, cause: TeardownCause) -> ActivationDriverError {
        if let Some(error) = self.latch_pending_exit() {
            return error;
        }
        if matches!(self.state, State::Running) {
            self.pump.force_teardown();
            self.transport.close();
            let _ = self.control.request_termination(tree_exit(cause.clone()));
            self.state = State::Teardown(cause.clone());
        }
        ActivationDriverError::TeardownRequired(cause)
    }
}
