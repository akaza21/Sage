use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    contract::RequiredText,
    runtime::{AdapterBatchJournal, HostCancelIntent},
};

use super::{
    CancelIntent, CancelWireState, DriverError, PreparedProcessWrite, PreparedWriteError,
    PreparedWriteFailure, ProcessTreeControl, StdinTransport,
};

impl<A, J, R, T, C> PreparedProcessWrite<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) async fn begin_cancel(
        mut self,
        intent: &HostCancelIntent,
    ) -> Result<Self, Box<PreparedWriteFailure<Self>>> {
        if intent.owner != *self.turn.driver.runtime_owner()
            || intent.turn_id != *self.turn.dispatch.turn_id()
        {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: PreparedWriteError::Intent,
            }));
        }
        if let Some(committed) = &self.cancel_intent {
            if committed == intent {
                return Ok(self);
            }
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: PreparedWriteError::Intent,
            }));
        }
        if let Some(cursor) = self.turn.driver.latest_durable_turn_cursor().cloned()
            && let Err(error) = self.turn.dispatch.accept_adapter_commit(&cursor)
        {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        if self.terminal_is_durable() {
            self.cancel_intent = Some(intent.clone());
            return Ok(self);
        }
        if let Err(error) = self
            .turn
            .dispatch
            .begin_cancel(intent.client_intent_id.clone(), intent.now_millis)
            .await
        {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        self.cancel_intent = Some(intent.clone());
        Ok(self)
    }

    pub(crate) fn dispatch_cancel(
        mut self,
        intent: &HostCancelIntent,
    ) -> Result<Self, Box<PreparedWriteFailure<Self>>> {
        if intent.owner != *self.turn.driver.runtime_owner()
            || intent.turn_id != *self.turn.dispatch.turn_id()
        {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: PreparedWriteError::Intent,
            }));
        }
        if self.cancel_intent.as_ref() != Some(intent) {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: PreparedWriteError::Intent,
            }));
        }
        if self.terminal_is_durable() {
            return Ok(self);
        }
        if self.cancel_wire != CancelWireState::NotAttempted {
            return Ok(self);
        }
        let client_message_id =
            match RequiredText::new("clientMessageId", intent.turn_id.as_str().to_owned()) {
                Ok(value) => value,
                Err(_) => {
                    return Err(Box::new(PreparedWriteFailure {
                        state: self,
                        error: PreparedWriteError::Intent,
                    }));
                }
            };
        loop {
            self.cancel_attempts = self.cancel_attempts.saturating_add(1);
            let progress = self.turn.driver.dispatch_cancel(CancelIntent {
                project_id: intent.owner.project_id.clone(),
                chat_id: intent.owner.chat_id.clone(),
                session_id: intent.owner.session_id.clone(),
                runtime_epoch: intent.owner.runtime_epoch,
                turn_id: intent.turn_id.clone(),
                client_message_id: client_message_id.clone(),
            });
            match progress {
                Ok(progress)
                    if progress.written_frames == 1 && progress.zero_byte_failures == 0 =>
                {
                    self.cancel_wire = CancelWireState::Complete;
                    return Ok(self);
                }
                Ok(progress)
                    if progress.written_frames == 0
                        && progress.zero_byte_failures == 1
                        && self.cancel_attempts < 2 =>
                {
                    continue;
                }
                Ok(progress)
                    if progress.written_frames == 0 && progress.zero_byte_failures == 1 =>
                {
                    let _ = self.turn.driver.request_host_termination();
                    self.cancel_wire = CancelWireState::Teardown;
                    return Ok(self);
                }
                Err(DriverError::TeardownRequired(_)) => {
                    self.cancel_wire = CancelWireState::Teardown;
                    return Ok(self);
                }
                Ok(_) => {
                    return Err(Box::new(PreparedWriteFailure {
                        state: self,
                        error: PreparedWriteError::Intent,
                    }));
                }
                Err(error) => {
                    return Err(Box::new(PreparedWriteFailure {
                        state: self,
                        error: PreparedWriteError::Driver(error),
                    }));
                }
            }
        }
    }

    pub(crate) fn cancel_intent(&self) -> Option<&HostCancelIntent> {
        self.cancel_intent.as_ref()
    }

    pub(crate) fn cancel_was_dispatched(&self) -> bool {
        self.cancel_wire == CancelWireState::Complete
    }

    pub(crate) fn cancel_requires_convergence(&self) -> bool {
        self.cancel_wire == CancelWireState::Teardown
    }

    fn terminal_is_durable(&self) -> bool {
        self.turn
            .driver
            .latest_durable_turn_cursor()
            .is_some_and(|cursor| {
                cursor.turn_id() == self.turn.dispatch.turn_id() && cursor.state().is_terminal()
            })
    }
}
