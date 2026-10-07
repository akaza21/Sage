use serde_json::Value;

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
        self.run_next_step_until(mailbox, None).await
    }

    pub(crate) async fn run_next_step_until(
        &mut self,
        mailbox: &DriverMailbox,
        operation_deadline_millis: Option<i64>,
    ) -> Result<DriverStep, DriverError> {
        self.ensure_running()?;
        self.settle_due_exit(self.clock.now_millis()).await?;
        let deadline = [
            self.pump.next_deferred_flush_deadline_millis(),
            operation_deadline_millis,
            self.pending_exit_deadline_millis(),
        ]
        .into_iter()
        .flatten()
        .min();
        let message = mailbox
            .wait_next_until(deadline, self.clock.as_ref())
            .map_err(DriverError::Mailbox)?;
        match message {
            Some(message) => {
                let now_millis = self.clock.now_millis();
                self.dispatch_step(message, mailbox, now_millis).await
            }
            None => {
                let now_millis = self.clock.now_millis();
                self.settle_due_exit(now_millis).await?;
                let progress = self.flush_due(now_millis).await?;
                Ok(DriverStep::Progress(progress))
            }
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
            None,
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
            ControlMessage::Prompt(intent) => {
                if self.pending_exit.is_some() {
                    let error = self.settle_exit(now_millis).await;
                    intent.reject(error.clone());
                    return Err(error);
                }
                self.prompt(intent).map(DriverStep::Progress)
            }
            ControlMessage::Cancel(intent) => {
                if self.pending_exit.is_some() {
                    return Err(self.settle_exit(now_millis).await);
                }
                self.cancel(intent, DispatchSource::Cancel)
                    .await
                    .map(DriverStep::Progress)
            }
            ControlMessage::Worker(envelope) => Ok(DriverStep::YieldWorker {
                progress: DriverProgress::default(),
                envelope,
            }),
            ControlMessage::ReaderTerminal(terminal) => match terminal {
                ReaderTerminal::Eof => {
                    self.observe_reader_eof(now_millis).await?;
                    Ok(DriverStep::Progress(DriverProgress::default()))
                }
                ReaderTerminal::Io => Err(self
                    .latch_after_reader_terminal(TeardownCause::ReaderIo, now_millis)
                    .await),
            },
            ControlMessage::ProcessExit(exit) => {
                self.observe_process_exit(exit, now_millis).await?;
                Ok(DriverStep::Progress(DriverProgress::default()))
            }
        }
    }

    pub(crate) async fn observe_reader_eof(&mut self, now_millis: i64) -> Result<(), DriverError> {
        if self.pending_exit.is_some() {
            return Err(self.settle_exit(now_millis).await);
        }
        let _ = self.finish_stdout(now_millis).await;
        self.record_reader_eof(now_millis);
        Ok(())
    }

    pub(crate) async fn observe_process_exit(
        &mut self,
        exit: crate::acp::process::TreeExit,
        now_millis: i64,
    ) -> Result<(), DriverError> {
        let reader_eof_pending = self
            .pending_exit
            .as_ref()
            .is_some_and(|pending| pending.exit.is_none());
        self.record_exit(exit, now_millis);
        if reader_eof_pending {
            return Err(self.settle_exit(now_millis).await);
        }
        Ok(())
    }

    fn prompt(
        &mut self,
        intent: crate::acp::runtime::process_driver::PromptIntent,
    ) -> Result<DriverProgress, DriverError> {
        let result = self.dispatch_prompt(intent.prompt, intent.client_message_id);
        let returned = result;
        let _ = intent.reply.send(returned.clone());
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
        let (produced, nonfatal_refusal) = match self
            .pump
            .push_stdout_chunk_deferred(chunk, now_millis)
            .await
        {
            Ok(produced) => (produced, None),
            Err(error @ crate::acp::runtime::AdapterPumpError::Refused(refusal)) => {
                self.log_rejected_inbound_frame(chunk, &error);
                (false, Some(refusal))
            }
            Err(error) => {
                self.log_rejected_inbound_frame(chunk, &error);
                return Err(self.pump_error(error));
            }
        };
        self.drain_buffered(
            mailbox,
            now_millis,
            DriverProgress::default(),
            produced,
            nonfatal_refusal,
        )
        .await
    }

    async fn drain_buffered(
        &mut self,
        mailbox: &DriverMailbox,
        now_millis: i64,
        mut progress: DriverProgress,
        mut produced: bool,
        mut nonfatal_refusal: Option<crate::acp::runtime::NonfatalRefusal>,
    ) -> Result<DriverStep, DriverError> {
        loop {
            if produced {
                progress.accepted_frames += 1;
                self.flush_all_ingress(DispatchSource::Ingress, now_millis, &mut progress)
                    .await?;
            }
            if nonfatal_refusal.is_none()
                && let Some(envelope) = self.dispatch_urgent(mailbox, &mut progress).await?
            {
                return Ok(DriverStep::YieldWorker { progress, envelope });
            }
            if nonfatal_refusal.is_some() {
                loop {
                    let produced = match self
                        .pump
                        .drain_stdout_after_config_rejection(now_millis)
                        .await
                    {
                        Ok(produced) => produced,
                        Err(error) => return Err(self.pump_error(error)),
                    };
                    if produced {
                        progress.accepted_frames += 1;
                        self.flush_all_ingress(DispatchSource::Ingress, now_millis, &mut progress)
                            .await?;
                    }
                    if !self.pump.has_pending_stdout_input() {
                        break;
                    }
                }
                break;
            }
            produced = match self.pump.drain_stdout_deferred(now_millis).await {
                Ok(produced) => produced,
                Err(crate::acp::runtime::AdapterPumpError::Refused(refusal)) => {
                    nonfatal_refusal = Some(refusal);
                    false
                }
                Err(error) => return Err(self.pump_error(error)),
            };
            if !produced {
                break;
            }
        }
        nonfatal_refusal.map_or(Ok(DriverStep::Progress(progress)), |refusal| {
            Err(DriverError::Refused(refusal))
        })
    }

    pub(super) async fn finish_stdout(&mut self, now_millis: i64) -> Result<(), DriverError> {
        loop {
            let produced = match self.pump.finish_stdout_deferred(now_millis).await {
                Ok(produced) => produced,
                Err(error) => return Err(self.pump_error(error)),
            };
            if !produced {
                return Ok(());
            }
            let mut progress = DriverProgress::default();
            self.flush_all_ingress(DispatchSource::Ingress, now_millis, &mut progress)
                .await?;
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
            self.flush_all_ingress(DispatchSource::Ingress, now_millis, &mut progress)
                .await?;
        }
        Ok(progress)
    }

    async fn dispatch_urgent(
        &mut self,
        mailbox: &DriverMailbox,
        progress: &mut DriverProgress,
    ) -> Result<Option<WorkerEnvelope>, DriverError> {
        while let Some(control) = mailbox.try_pop_control() {
            match control {
                ControlMessage::Cancel(intent) => {
                    if self.pending_exit.is_some() {
                        return Err(self.settle_exit(self.clock.now_millis()).await);
                    }
                    match self.cancel(intent, DispatchSource::Cancel).await {
                        Ok(current) => {
                            progress.written_frames += current.written_frames;
                            progress.cancelled_permission_frames +=
                                current.cancelled_permission_frames;
                            progress.zero_byte_failures += current.zero_byte_failures;
                        }
                        Err(DriverError::RejectedIntent | DriverError::IdentityMismatch) => {
                            progress.rejected_controls += 1;
                        }
                        Err(error) => return Err(error),
                    }
                }
                ControlMessage::ReaderTerminal(_) => {
                    if self.pending_exit.is_some() {
                        return Err(self.settle_exit(self.clock.now_millis()).await);
                    }
                    return Err(self.latch(TeardownCause::ProtocolFailure));
                }
                ControlMessage::ProcessExit(exit) => {
                    self.record_exit(exit, self.clock.now_millis());
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

    fn log_rejected_inbound_frame(
        &self,
        chunk: &[u8],
        error: &crate::acp::runtime::AdapterPumpError,
    ) {
        const DIAGNOSTIC_BYTES: usize = 1024;

        let truncated = chunk.len() > DIAGNOSTIC_BYTES;
        let preview = &chunk[..chunk.len().min(DIAGNOSTIC_BYTES)];
        let parsed = serde_json::from_slice::<Value>(preview).ok();
        let method = parsed
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|object| object.get("method"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let id = parsed
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|object| object.get("id"))
            .and_then(Value::as_u64);
        let rpc_error_code = parsed
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|object| object.get("error"))
            .and_then(Value::as_object)
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64);
        let frame_json = parsed
            .as_ref()
            .map(|value| redact_frame_value(value.clone()))
            .and_then(|value| serde_json::to_string(&value).ok())
            .unwrap_or_else(|| format!("<non-json-or-fragment bytes={}>", chunk.len()));
        tracing::warn!(
            activation_checkpoint = "process_driver.ingress.reject",
            ?error,
            json_rpc_method = method,
            json_rpc_id = id,
            json_rpc_error_code = rpc_error_code,
            frame_json = %frame_json,
            frame_truncated = truncated,
            "ACP process driver rejected inbound frame"
        );
    }
}

fn redact_frame_value(value: Value) -> Value {
    match value {
        Value::String(value) => Value::String(format!("<redacted:{}>", value.len())),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_frame_value).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let value = if matches!(key.as_str(), "method" | "status") {
                        value
                    } else {
                        redact_frame_value(value)
                    };
                    (key, value)
                })
                .collect(),
        ),
        value => value,
    }
}
