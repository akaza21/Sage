use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::AdapterBatchJournal,
};

use super::{DriverStep, ProcessAdapterDriver};
use crate::acp::runtime::process_driver::{
    ControlMessage, DispatchSource, DriverError, DriverMailbox, DriverProgress, ProcessTreeControl,
    ReaderTerminal, StdinTransport, TeardownCause, TreeLeaseOwner, WorkerEnvelope,
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
    pub(crate) async fn run_next(
        &mut self,
        mailbox: &DriverMailbox,
    ) -> Result<DriverProgress, DriverError> {
        match self.run_next_step(mailbox).await? {
            DriverStep::Progress(progress) => Ok(progress),
            DriverStep::YieldWorker { envelope, .. } => {
                envelope.reject(crate::acp::runtime::HostError::State);
                Err(DriverError::RejectedIntent)
            }
        }
    }

    pub(crate) async fn run_next_step(
        &mut self,
        mailbox: &DriverMailbox,
    ) -> Result<DriverStep, DriverError> {
        self.ensure_running()?;
        let deadline = self.pump.next_deferred_flush_deadline_millis();
        let message = mailbox
            .wait_next_until(deadline, self.clock.as_ref())
            .map_err(DriverError::Mailbox)?;
        match message {
            Some(message) => {
                let now_millis = self.clock.now_millis();
                self.dispatch_step(message, mailbox, now_millis).await
            }
            None => self
                .flush_due(self.clock.now_millis())
                .await
                .map(DriverStep::Progress),
        }
    }

    pub(crate) async fn resume_after_worker_control(
        &mut self,
        mailbox: &DriverMailbox,
    ) -> Result<DriverStep, DriverError> {
        self.ensure_running()?;
        self.drain_buffered(
            mailbox,
            self.clock.now_millis(),
            DriverProgress::default(),
            false,
        )
        .await
    }

    async fn dispatch_step(
        &mut self,
        message: ControlMessage,
        mailbox: &DriverMailbox,
        now_millis: i64,
    ) -> Result<DriverStep, DriverError> {
        match message {
            ControlMessage::StdoutChunk(chunk) => {
                self.accept_chunk(&chunk, mailbox, now_millis).await
            }
            ControlMessage::Prompt(intent) => self.prompt(intent).map(DriverStep::Progress),
            ControlMessage::Cancel(intent) => self
                .cancel(intent, DispatchSource::Cancel)
                .map(DriverStep::Progress),
            ControlMessage::Worker(envelope) => Ok(DriverStep::YieldWorker {
                progress: DriverProgress::default(),
                envelope,
            }),
            ControlMessage::ReaderTerminal(terminal) => match terminal {
                ReaderTerminal::Eof => {
                    self.finish_stdout(now_millis).await?;
                    Err(self.latch(TeardownCause::ReaderEof))
                }
                ReaderTerminal::Io => Err(self.latch(TeardownCause::ReaderIo)),
            },
            ControlMessage::ProcessExit => Err(self.latch(TeardownCause::ProcessExit)),
        }
    }

    fn prompt(
        &mut self,
        intent: crate::acp::runtime::process_driver::PromptIntent,
    ) -> Result<DriverProgress, DriverError> {
        let result = self.dispatch_prompt(intent.prompt, intent.client_message_id);
        let returned = result;
        let _ = intent.reply.send(returned);
        match returned {
            Err(error @ DriverError::TeardownRequired(_)) => Err(error),
            _ => Ok(DriverProgress::default()),
        }
    }

    async fn accept_chunk(
        &mut self,
        chunk: &[u8],
        mailbox: &DriverMailbox,
        now_millis: i64,
    ) -> Result<DriverStep, DriverError> {
        let produced = match self
            .pump
            .push_stdout_chunk_deferred(chunk, now_millis)
            .await
        {
            Ok(produced) => produced,
            Err(error) => return Err(self.pump_error(error)),
        };
        self.drain_buffered(mailbox, now_millis, DriverProgress::default(), produced)
            .await
    }

    async fn drain_buffered(
        &mut self,
        mailbox: &DriverMailbox,
        now_millis: i64,
        mut progress: DriverProgress,
        mut produced: bool,
    ) -> Result<DriverStep, DriverError> {
        loop {
            if produced {
                progress.accepted_frames += 1;
                self.flush_all(DispatchSource::Ingress, &mut progress)?;
            }
            if let Some(envelope) = self.dispatch_urgent(mailbox, &mut progress)? {
                return Ok(DriverStep::YieldWorker { progress, envelope });
            }
            produced = match self.pump.drain_stdout_deferred(now_millis).await {
                Ok(produced) => produced,
                Err(error) => return Err(self.pump_error(error)),
            };
            if !produced {
                break;
            }
        }
        Ok(DriverStep::Progress(progress))
    }

    async fn finish_stdout(&mut self, now_millis: i64) -> Result<(), DriverError> {
        loop {
            let produced = match self.pump.finish_stdout_deferred(now_millis).await {
                Ok(produced) => produced,
                Err(error) => return Err(self.pump_error(error)),
            };
            if !produced {
                return Ok(());
            }
            let mut progress = DriverProgress::default();
            self.flush_all(DispatchSource::Ingress, &mut progress)?;
        }
    }

    async fn flush_due(&mut self, now_millis: i64) -> Result<DriverProgress, DriverError> {
        let mut progress = DriverProgress::default();
        let flushed = match self.pump.flush_deferred_if_needed(now_millis).await {
            Ok(flushed) => flushed,
            Err(error) => return Err(self.pump_error(error)),
        };
        if flushed {
            progress.deferred_flushes = 1;
            self.flush_all(DispatchSource::Ingress, &mut progress)?;
        }
        Ok(progress)
    }

    fn dispatch_urgent(
        &mut self,
        mailbox: &DriverMailbox,
        progress: &mut DriverProgress,
    ) -> Result<Option<WorkerEnvelope>, DriverError> {
        while let Some(control) = mailbox.try_pop_control() {
            match control {
                ControlMessage::Cancel(intent) => {
                    match self.cancel(intent, DispatchSource::Cancel) {
                        Ok(current) => {
                            progress.written_frames += current.written_frames;
                            progress.zero_byte_failures += current.zero_byte_failures;
                        }
                        Err(DriverError::RejectedIntent | DriverError::IdentityMismatch) => {
                            progress.rejected_controls += 1;
                        }
                        Err(error) => return Err(error),
                    }
                }
                ControlMessage::ReaderTerminal(_) => {
                    return Err(self.latch(TeardownCause::ProtocolFailure));
                }
                ControlMessage::ProcessExit => {
                    return Err(self.latch(TeardownCause::ProcessExit));
                }
                ControlMessage::StdoutChunk(_) => {
                    return Err(self.latch(TeardownCause::ProtocolFailure));
                }
                ControlMessage::Prompt(intent) => {
                    intent.reject(DriverError::RejectedIntent);
                    progress.rejected_controls += 1;
                }
                ControlMessage::Worker(envelope) => return Ok(Some(envelope)),
            }
        }
        Ok(None)
    }
}
