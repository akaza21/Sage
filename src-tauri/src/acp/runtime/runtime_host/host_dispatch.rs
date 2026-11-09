use super::*;

impl RuntimeHost {
    pub(crate) fn try_notify_terminal(
        &self,
        owner: RuntimeOwner,
        cursor: AdapterTurnCursor,
    ) -> Result<TerminalNotification, HostError> {
        match self.lifecycle.load(Ordering::Acquire) {
            HOST_RUNNING => {}
            HOST_AWAITING_EXIT => return Err(HostError::RecoveryRequired),
            HOST_SHUTDOWN_ACKNOWLEDGED | HOST_JOINED => return Err(HostError::Closed),
            _ => return Err(HostError::RecoveryRequired),
        }
        let command = HostCommand::Terminal { owner, cursor };
        let timeout = self.response_timeout(&command);
        if timeout.is_zero() {
            self.fail_deadline();
            return Err(HostError::Deadline);
        }
        let deadline = Instant::now() + timeout;
        let (reply, response) = mpsc::sync_channel(1);
        self.mailbox.try_push(Envelope {
            command,
            reply,
            deadline,
        })?;
        Ok(TerminalNotification { response, deadline })
    }

    pub(super) fn outcome(&self, command: HostCommand) -> Result<HostOutcome, HostError> {
        match self.submit(command)? {
            HostReply::Outcome(outcome) => Ok(outcome),
            HostReply::NativeRpc(_) | HostReply::Prepared(_) | HostReply::State(_) => {
                Err(HostError::State)
            }
        }
    }

    pub(in crate::acp::runtime::runtime_host) fn outcome_with_timeout(
        &self,
        command: HostCommand,
        timeout: Duration,
    ) -> Result<HostOutcome, HostError> {
        match self.submit_with_timeout(command, timeout)? {
            HostReply::Outcome(outcome) => Ok(outcome),
            HostReply::NativeRpc(_) | HostReply::Prepared(_) | HostReply::State(_) => {
                Err(HostError::State)
            }
        }
    }

    pub(super) fn outcome_until(
        &self,
        command: HostCommand,
        deadline: Instant,
    ) -> Result<HostOutcome, HostError> {
        match self.submit_until(command, deadline)? {
            HostReply::Outcome(outcome) => Ok(outcome),
            HostReply::NativeRpc(_) | HostReply::Prepared(_) | HostReply::State(_) => {
                Err(HostError::State)
            }
        }
    }

    pub(super) fn submit(&self, command: HostCommand) -> Result<HostReply, HostError> {
        let timeout = self.response_timeout(&command);
        self.submit_with_timeout(command, timeout)
    }

    pub(in crate::acp::runtime::runtime_host) fn submit_with_timeout(
        &self,
        command: HostCommand,
        timeout: Duration,
    ) -> Result<HostReply, HostError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(HostError::Deadline)?;
        self.submit_until(command, deadline)
    }

    pub(in crate::acp::runtime::runtime_host) fn submit_until(
        &self,
        command: HostCommand,
        deadline: Instant,
    ) -> Result<HostReply, HostError> {
        let actor_settles_deadline = matches!(
            &command,
            HostCommand::ConfigureSession(_) | HostCommand::NativeRpc(_) | HostCommand::Steer(_)
        );
        let steer = matches!(&command, HostCommand::Steer(_));
        match self.lifecycle.load(Ordering::Acquire) {
            HOST_RUNNING => {}
            HOST_AWAITING_EXIT => return Err(HostError::RecoveryRequired),
            HOST_SHUTDOWN_ACKNOWLEDGED | HOST_JOINED => return Err(HostError::Closed),
            _ => return Err(HostError::RecoveryRequired),
        }
        if Instant::now() >= deadline {
            // The actor refuses these unsent past their deadline too.
            if actor_settles_deadline && !steer {
                return Err(HostError::NotDispatched);
            }
            self.fail_deadline();
            return Err(HostError::Deadline);
        }
        let (reply, response) = mpsc::sync_channel(1);
        self.mailbox.try_push(Envelope {
            command,
            reply,
            deadline,
        })?;
        match response.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(reply) => reply,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !actor_settles_deadline {
                    self.fail_deadline();
                }
                Err(if steer {
                    HostError::SteerOutcomeUnknown
                } else {
                    HostError::Deadline
                })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.lifecycle.store(HOST_AWAITING_EXIT, Ordering::Release);
                self.mailbox.close();
                Err(HostError::Closed)
            }
        }
    }

    pub(super) fn fail_deadline(&self) {
        let _ = self.lifecycle.compare_exchange(
            HOST_RUNNING,
            HOST_AWAITING_EXIT,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.mailbox.close();
    }

    pub(super) fn response_timeout(&self, command: &HostCommand) -> Duration {
        #[cfg(test)]
        if self.timeout_override_suspended.load(Ordering::Acquire) {
            return Duration::from_millis(command.deadline_millis());
        }
        #[cfg(any(test, all(windows, feature = "acp-fixture")))]
        if let Some(timeout) = self.timeout_override {
            return match timeout {
                #[cfg(test)]
                TimeoutOverride::Duration(duration) => duration,
                TimeoutOverride::Deadline { operation, .. } => operation
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(command.deadline_millis())),
            };
        }
        Duration::from_millis(command.deadline_millis())
    }

    #[cfg(test)]
    pub(in crate::acp::runtime::runtime_host) fn join(&self) -> Result<(), HostError> {
        self.join_with_timeout(self.cleanup_timeout())
    }

    pub(super) fn cleanup_timeout(&self) -> Duration {
        let native = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        #[cfg(any(test, all(windows, feature = "acp-fixture")))]
        if let Some(TimeoutOverride::Deadline { cleanup, .. }) = self.timeout_override {
            return native.min(cleanup.saturating_duration_since(Instant::now()));
        }
        native
    }

    pub(in crate::acp::runtime::runtime_host) fn join_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(), HostError> {
        let mut thread = self
            .thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(worker) = thread.as_mut() else {
            return Ok(());
        };
        match worker.wait(timeout) {
            Ok(()) => {
                *thread = None;
                self.lifecycle.store(HOST_JOINED, Ordering::Release);
                Ok(())
            }
            Err(ThreadWaitError::Timeout) => Err(HostError::Deadline),
            Err(ThreadWaitError::Panicked) => {
                *thread = None;
                self.lifecycle.store(HOST_JOINED, Ordering::Release);
                Err(HostError::Unavailable)
            }
        }
    }
}
