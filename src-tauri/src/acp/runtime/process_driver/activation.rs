use std::sync::Arc;

mod handoff;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{ActivationFlush, ActivationJournal, ActivationPump, ActivationPumpError},
    scheduler::RuntimeOwner,
};

use super::types::tree_exit;
use super::{
    ControlMessage, DispatchSource, DriverClock, DriverMailbox, DriverProgress,
    FirstDeliveryUnknown, ProcessTreeControl, ReaderTerminal, StdinTransport, TeardownCause,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Running,
    ProjectionRecovery,
    Teardown(TeardownCause),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
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
}

impl<A, J, R, T, C> ActivationProcessDriver<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: ActivationJournal,
    R: ResourceMapper,
    T: StdinTransport + crate::acp::runtime::ActivationTransport,
    C: ProcessTreeControl,
{
    pub(crate) fn new(
        pump: ActivationPump<A, J, R>,
        transport: T,
        control: C,
        owner: RuntimeOwner,
        clock: Arc<dyn DriverClock>,
    ) -> Result<Self, ActivationDriverError> {
        if pump.runtime_owner() != owner
            || owner.runtime_epoch == crate::acp::contract::RuntimeEpoch::ZERO
        {
            return Err(ActivationDriverError::IdentityMismatch);
        }
        Ok(Self {
            pump,
            transport,
            control,
            owner,
            clock,
            state: State::Running,
            first_delivery_unknown: None,
        })
    }

    pub(crate) fn begin(&mut self) -> Result<DriverProgress, ActivationDriverError> {
        self.ensure_running()?;
        self.pump.begin().map_err(|error| self.pump_error(error))?;
        let mut progress = DriverProgress::default();
        self.flush_all(&mut progress)?;
        Ok(progress)
    }

    pub(crate) async fn run_next(
        &mut self,
        mailbox: &DriverMailbox,
    ) -> Result<DriverProgress, ActivationDriverError> {
        self.ensure_running()?;
        let message = mailbox
            .wait_next_until(None, self.clock.as_ref())
            .map_err(|_| ActivationDriverError::Mailbox)?
            .ok_or(ActivationDriverError::Mailbox)?;
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
                Err(self.latch(TeardownCause::ReaderEof))
            }
            ControlMessage::ReaderTerminal(ReaderTerminal::Io) => {
                Err(self.latch(TeardownCause::ReaderIo))
            }
            ControlMessage::ProcessExit => Err(self.latch(TeardownCause::ProcessExit)),
        }
    }

    async fn accept_chunk(
        &mut self,
        chunk: &[u8],
        mailbox: &DriverMailbox,
        now_millis: i64,
    ) -> Result<DriverProgress, ActivationDriverError> {
        let mut progress = DriverProgress::default();
        let mut produced = self
            .pump
            .push_stdout_chunk(chunk, now_millis)
            .await
            .map_err(|error| self.pump_error(error))?;
        loop {
            if produced {
                progress.accepted_frames += 1;
                self.flush_all(&mut progress)?;
            }
            self.dispatch_urgent(mailbox, &mut progress)?;
            produced = self
                .pump
                .drain_stdout(now_millis)
                .await
                .map_err(|error| self.pump_error(error))?;
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
                ControlMessage::ProcessExit => {
                    return Err(self.latch(TeardownCause::ProcessExit));
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
                    return Err(self.latch(TeardownCause::DeliveryUnknown));
                }
                Err(error) => return Err(self.pump_error(error)),
            }
        }
    }

    fn pump_error(&mut self, error: ActivationPumpError) -> ActivationDriverError {
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
            ActivationPumpError::Adapter(
                crate::acp::bindings::adapter::AdapterRuntimeError::MandatoryDenyTeardown,
            ) => TeardownCause::MandatoryDenyFailure,
            _ => TeardownCause::PumpFailure,
        })
    }

    fn ensure_running(&self) -> Result<(), ActivationDriverError> {
        match self.state {
            State::Running => Ok(()),
            State::ProjectionRecovery => Err(ActivationDriverError::ProjectionRecoveryRequired),
            State::Teardown(cause) => Err(ActivationDriverError::TeardownRequired(cause)),
        }
    }

    fn latch(&mut self, cause: TeardownCause) -> ActivationDriverError {
        if matches!(self.state, State::Running) {
            self.pump.force_teardown();
            self.transport.close();
            let _ = self.control.request_termination(tree_exit(cause));
            self.state = State::Teardown(cause);
        }
        ActivationDriverError::TeardownRequired(cause)
    }
}
