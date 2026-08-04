use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    contract::RequiredText,
    intent::Prompt,
    runtime::{AdapterBatchJournal, CoordinatorError, HostCancelIntent, LiveWrite},
    store::{AdapterTurnCursor, SendObservation},
};

use super::{
    CancelIntent, CompletedDriverParts, DriverError, DriverMailbox, PreparedDriverTurn,
    ProcessTreeControl, RebindFailure, StdinTransport, WriteFence, WriteFenceError,
};

mod cancel;

#[derive(Debug, thiserror::Error)]
pub(crate) enum PreparedWriteError {
    #[error("the prepared prompt identity is invalid")]
    Intent,
    #[error("the durable Turn transition failed")]
    Coordinator(#[from] CoordinatorError),
    #[error("the process write failed")]
    Driver(DriverError),
    #[error("the write observation cannot cross this fence")]
    Fence(#[from] WriteFenceError),
}

pub(crate) struct PreparedWriteFailure<S> {
    pub(crate) state: S,
    pub(crate) error: PreparedWriteError,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CancelWireState {
    NotAttempted,
    Complete,
    Teardown,
}

pub(crate) struct PreparedProcessWrite<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) turn: PreparedDriverTurn<A, J, R, T, C>,
    prompt: Prompt,
    fence: WriteFence,
    cancel_intent: Option<HostCancelIntent>,
    cancel_wire: CancelWireState,
    cancel_attempts: u8,
    durable_mutation_unknown: bool,
}

impl<A, J, R, T, C> PreparedProcessWrite<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) fn new(turn: PreparedDriverTurn<A, J, R, T, C>, prompt: Prompt) -> Self {
        let turn_id = turn.dispatch.turn_id();
        debug_assert_ne!(
            turn.driver.runtime_owner().runtime_epoch,
            crate::acp::contract::RuntimeEpoch::ZERO
        );
        Self {
            fence: WriteFence::bound(turn_id.clone()),
            turn,
            prompt,
            cancel_intent: None,
            cancel_wire: CancelWireState::NotAttempted,
            cancel_attempts: 0,
            durable_mutation_unknown: false,
        }
    }

    pub(crate) fn mark_durable_mutation_unknown(mut self) -> Self {
        self.durable_mutation_unknown = true;
        let _ = self.turn.driver.request_host_termination();
        self
    }

    pub(crate) fn durable_mutation_unknown(&self) -> bool {
        self.durable_mutation_unknown
    }

    pub(crate) async fn reconcile_durable_mutation_unknown(
        mut self,
        now_millis: i64,
    ) -> Result<(Self, crate::acp::store::TurnMutation), Box<PreparedWriteFailure<Self>>> {
        if !self.durable_mutation_unknown {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: PreparedWriteError::Intent,
            }));
        }
        let mutation = match self
            .turn
            .dispatch
            .reconcile_commit_unknown(now_millis)
            .await
        {
            Ok(mutation) => mutation,
            Err(error) => {
                return Err(Box::new(PreparedWriteFailure {
                    state: self,
                    error: error.into(),
                }));
            }
        };
        self.durable_mutation_unknown = false;
        Ok((self, mutation))
    }

    pub(crate) async fn dispatch_once(
        mut self,
        now_millis: i64,
    ) -> Result<(Self, LiveWrite), Box<PreparedWriteFailure<Self>>> {
        if let Err(error) = self.turn.dispatch.begin_send(now_millis).await {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        self.write_prompt()
    }

    pub(crate) fn retry_send_projection(
        mut self,
    ) -> Result<(Self, LiveWrite), Box<PreparedWriteFailure<Self>>> {
        if let Err(error) = self.turn.dispatch.retry_projection() {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        self.write_prompt()
    }

    fn write_prompt(mut self) -> Result<(Self, LiveWrite), Box<PreparedWriteFailure<Self>>> {
        let turn_id = self.turn.dispatch.turn_id().clone();
        if let Err(error) = self.fence.begin_write(&turn_id) {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        let client_message_id =
            match RequiredText::new("clientMessageId", turn_id.as_str().to_owned()) {
                Ok(value) => value,
                Err(_) => {
                    return Err(Box::new(PreparedWriteFailure {
                        state: self,
                        error: PreparedWriteError::Intent,
                    }));
                }
            };
        let result = self
            .turn
            .driver
            .dispatch_prompt(self.prompt.clone(), client_message_id);
        let write = match result {
            Ok(progress) if progress.written_frames == 1 && progress.zero_byte_failures == 0 => {
                LiveWrite::Complete
            }
            Ok(progress) if progress.written_frames == 0 && progress.zero_byte_failures == 1 => {
                LiveWrite::ZeroBytes
            }
            Err(DriverError::TeardownRequired(_))
                if self.turn.driver.first_delivery_unknown().is_some() =>
            {
                LiveWrite::PossibleBytes
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
        };
        Ok((self, write))
    }

    pub(crate) async fn observe(
        mut self,
        observation: SendObservation,
        now_millis: i64,
    ) -> Result<Self, Box<PreparedWriteFailure<Self>>> {
        if self.durable_mutation_unknown {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: PreparedWriteError::Intent,
            }));
        }
        let turn_id = self.turn.dispatch.turn_id().clone();
        let durable = self.turn.dispatch.observe(observation, now_millis).await;
        if let Err(error) = durable {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        if let Err(error) = self.fence.accept_observation(&turn_id, observation) {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        Ok(self)
    }

    pub(crate) fn projection_retry_pending(&self) -> bool {
        self.turn.dispatch.projection_retry_pending()
    }

    pub(crate) fn retry_observation_projection(
        mut self,
        observation: SendObservation,
    ) -> Result<Self, Box<PreparedWriteFailure<Self>>> {
        let turn_id = self.turn.dispatch.turn_id().clone();
        if let Err(error) = self.turn.dispatch.retry_projection() {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        if let Err(error) = self.fence.accept_observation(&turn_id, observation) {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        Ok(self)
    }

    pub(crate) fn accept_terminal(
        mut self,
        cursor: &AdapterTurnCursor,
    ) -> Result<Self, Box<PreparedWriteFailure<Self>>> {
        if !cursor.state().is_terminal() || cursor.turn_id() != self.turn.dispatch.turn_id() {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: PreparedWriteError::Intent,
            }));
        }
        if let Err(error) = self.turn.dispatch.accept_adapter_commit(cursor) {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        Ok(self)
    }

    pub(crate) fn accept_latest_durable_terminal(
        mut self,
    ) -> Result<Self, Box<PreparedWriteFailure<Self>>> {
        let Some(cursor) = self.turn.driver.latest_durable_turn_cursor().cloned() else {
            return Ok(self);
        };
        if !cursor.state().is_terminal() {
            return Ok(self);
        }
        if let Err(error) = self.turn.dispatch.accept_adapter_commit(&cursor) {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        Ok(self)
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn try_into_completed(
        self,
        mailbox: &DriverMailbox,
    ) -> Result<
        CompletedDriverParts<A, J, R, T, C, crate::acp::runtime::SessionTreeLease>,
        Box<PreparedWriteFailure<Self>>,
    > {
        let Self {
            turn,
            prompt,
            fence,
            cancel_intent,
            cancel_wire,
            cancel_attempts,
            durable_mutation_unknown,
        } = self;
        let PreparedDriverTurn { driver, dispatch } = turn;
        match driver.try_complete_turn(mailbox) {
            Ok(completed) => Ok(completed),
            Err(failure) => {
                let RebindFailure { driver, error } = *failure;
                Err(Box::new(PreparedWriteFailure {
                    state: Self {
                        turn: PreparedDriverTurn { driver, dispatch },
                        prompt,
                        fence,
                        cancel_intent,
                        cancel_wire,
                        cancel_attempts,
                        durable_mutation_unknown,
                    },
                    error: PreparedWriteError::Driver(error),
                }))
            }
        }
    }

    pub(crate) fn stdout_allowed(&self) -> bool {
        self.fence.stdout_allowed()
    }

    pub(crate) fn ready_for_write(&self) -> bool {
        !self.durable_mutation_unknown && self.fence.ready_for_write()
    }
}
