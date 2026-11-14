use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    contract::RequiredText,
    intent::Prompt,
    runtime::{AdapterBatchJournal, CoordinatorError, HostCancelIntent, LiveWrite},
    store::{AdapterTurnCursor, SendObservation},
};

use super::{
    CancelIntent, CompletedDriverParts, CoordinatorZeroToken, DriverError, DriverMailbox,
    PreparedDriverTurn, ProcessTreeControl, RebindFailure, StdinTransport, WriteFenceError,
};

mod cancel;
mod delivery;
mod steer;

use delivery::PromptDelivery;
use steer::PendingSteer;

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
    delivery: PromptDelivery,
    access_mode: crate::acp_protocol::access_mode::SessionAccessMode,
    cancel_intent: Option<HostCancelIntent>,
    cancel_wire: CancelWireState,
    cancel_attempts: u8,
    durable_mutation_unknown: bool,
    pending_steer: Option<PendingSteer>,
}

impl<A, J, R, T, C> PreparedProcessWrite<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) fn new(
        turn: PreparedDriverTurn<A, J, R, T, C>,
        prompt: Prompt,
        access_mode: crate::acp_protocol::access_mode::SessionAccessMode,
    ) -> Self {
        let turn_id = turn.dispatch.turn_id();
        debug_assert_ne!(
            turn.driver.runtime_owner().runtime_epoch,
            crate::acp::contract::RuntimeEpoch::ZERO
        );
        Self {
            delivery: PromptDelivery::new(turn_id.clone(), prompt),
            turn,
            access_mode,
            cancel_intent: None,
            cancel_wire: CancelWireState::NotAttempted,
            cancel_attempts: 0,
            durable_mutation_unknown: false,
            pending_steer: None,
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
        let prompt = match self.delivery.begin_write(&turn_id) {
            Ok(prompt) => prompt,
            Err(error) => {
                return Err(Box::new(PreparedWriteFailure {
                    state: self,
                    error: error.into(),
                }));
            }
        };
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
        let result = self.turn.driver.dispatch_prompt_with_access_mode(
            prompt,
            client_message_id,
            self.access_mode,
        );
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
        let terminal_reason = self.turn.driver.terminal_outcome();
        let durable = self
            .turn
            .dispatch
            .observe(observation, terminal_reason, now_millis)
            .await;
        if let Err(error) = durable {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        if let Err(error) = self.delivery.accept_observation(&turn_id, observation) {
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
        if let Err(error) = self.delivery.accept_observation(&turn_id, observation) {
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
            delivery,
            access_mode,
            cancel_intent,
            cancel_wire,
            cancel_attempts,
            durable_mutation_unknown,
            pending_steer,
        } = self;
        let PreparedDriverTurn { driver, dispatch } = turn;
        match driver.try_complete_turn(mailbox) {
            Ok(completed) => Ok(completed),
            Err(failure) => {
                let RebindFailure { driver, error } = *failure;
                Err(Box::new(PreparedWriteFailure {
                    state: Self {
                        turn: PreparedDriverTurn { driver, dispatch },
                        delivery,
                        access_mode,
                        cancel_intent,
                        cancel_wire,
                        cancel_attempts,
                        durable_mutation_unknown,
                        pending_steer,
                    },
                    error: PreparedWriteError::Driver(error),
                }))
            }
        }
    }

    pub(crate) fn stdout_allowed(&self) -> bool {
        self.delivery.fence().stdout_allowed()
    }

    pub(crate) fn awaiting_write_observation(&self) -> bool {
        self.delivery.fence().awaiting_observation()
    }

    pub(crate) fn ready_for_write(&self) -> bool {
        !self.durable_mutation_unknown && self.delivery.fence().ready_for_write()
    }

    pub(crate) fn has_durable_terminal(&self) -> bool {
        self.turn.dispatch.is_terminal()
    }

    pub(crate) fn durable_turn_state(&self) -> crate::acp::lifecycle::TurnState {
        self.turn.dispatch.state()
    }

    pub(crate) fn close_stdin(&mut self) {
        self.turn.driver.close_stdin();
    }

    pub(crate) fn write_close_session(&mut self) {
        let _ = self.turn.driver.dispatch_close_session();
    }

    pub(crate) async fn park_after_zero(&mut self, token: crate::acp::process::ZeroProcessToken)
    where
        C::ZeroProof: CoordinatorZeroToken,
    {
        let owner = self.turn.driver.owner.clone();
        let Ok(proof) = token.try_into_coordinator_proof(owner.clone()) else {
            return;
        };
        let Some(lease) = self.turn.driver.lease.as_mut() else {
            return;
        };
        let Ok(exit_kind) = crate::acp::contract::RequiredText::new("exitKind", "hostShutdown")
        else {
            return;
        };
        let _ = lease
            .park_after_zero_process(
                crate::acp::store::ParkSessionCommand {
                    project_id: owner.project_id.clone(),
                    chat_id: owner.chat_id.clone(),
                    session_id: owner.session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    exit_kind,
                    now_millis: sage_core::time::now_millis(),
                },
                proof,
            )
            .await;
    }
}
