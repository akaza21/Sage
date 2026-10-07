use std::sync::Arc;
#[cfg(all(test, windows))]
use std::time::Duration;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::{LaunchError, TreeExit, ZeroProcessToken},
    runtime::{
        ActiveDispatch, ActiveTurnIntent, AdapterBatchJournal, CompletedPumpParts, PrepareDispatch,
        PrepareRecovery, ProjectIntentAuthority, SendCoordinator, SessionTreeLease,
        ZeroProcessProof,
    },
    scheduler::RuntimeOwner,
    store::PrepareTurnCommand,
};

use super::{
    CoordinatorZeroToken, DriverClock, DriverError, DriverMailbox, ProcessAdapterDriver,
    ProcessTreeControl, StdinTransport, TeardownCause, TreeLeaseOwner, driver::DriverState,
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
    pub(crate) fn try_complete_turn(
        self,
        mailbox: &DriverMailbox,
    ) -> CompleteTurnResult<A, J, R, T, C, L> {
        if !matches!(self.state, DriverState::Running)
            || self.first_delivery_unknown.is_some()
            || self.pending_exit.is_some()
            || !mailbox.is_quiescent()
            || !self.pump.completed_turn_ready()
        {
            return Err(Box::new(RebindFailure {
                driver: self,
                error: DriverError::NotReusable,
            }));
        }
        let ProcessAdapterDriver {
            pump,
            transport,
            control,
            lease,
            owner,
            clock,
            state,
            first_delivery_unknown,
            durable_mutation_unknown,
            teardown_detail,
            pending_exit,
        } = self;
        let completed = match pump.try_into_completed_parts() {
            Ok(value) => value,
            Err(error) => {
                let (pump, _) = *error;
                return Err(Box::new(RebindFailure {
                    driver: ProcessAdapterDriver {
                        pump,
                        transport,
                        control,
                        lease,
                        owner,
                        clock,
                        state,
                        first_delivery_unknown,
                        durable_mutation_unknown,
                        teardown_detail,
                        pending_exit,
                    },
                    error: DriverError::NotReusable,
                }));
            }
        };
        Ok(CompletedDriverParts {
            completed,
            transport,
            control,
            lease,
            owner,
            clock,
        })
    }

    pub(crate) fn take_zero_process_handoff(
        &mut self,
    ) -> Result<ZeroProcessHandoff<L, C::ZeroProof>, DriverError> {
        self.take_zero_process_handoff_after(ProcessTreeControl::wait_zero)
    }

    fn take_zero_process_handoff_after(
        &mut self,
        wait_zero: impl FnOnce(&C) -> Result<(TreeExit, C::ZeroProof), LaunchError>,
    ) -> Result<ZeroProcessHandoff<L, C::ZeroProof>, DriverError> {
        let cause = self.teardown_cause().ok_or(DriverError::Tree)?;
        if self.lease.is_none() {
            return Err(DriverError::Tree);
        }
        let (exit, proof) = wait_zero(&self.control).map_err(|_| DriverError::Tree)?;
        let lease = self.lease.take().ok_or(DriverError::Tree)?;
        Ok(ZeroProcessHandoff {
            lease,
            proof,
            exit,
            cause,
            first_delivery_unknown: self.first_delivery_unknown,
        })
    }
}

#[cfg(all(test, windows))]
impl<A, J, R, T, L> ProcessAdapterDriver<A, J, R, T, crate::acp::process::TreeControl, L>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    L: TreeLeaseOwner,
{
    pub(crate) fn take_zero_process_handoff_within(
        &mut self,
        timeout: Duration,
    ) -> Result<ZeroProcessHandoff<L, ZeroProcessToken>, DriverError> {
        self.take_zero_process_handoff_after(|control| control.wait_zero_within(timeout))
    }
}

pub(crate) struct RebindFailure<D> {
    pub(crate) driver: D,
    pub(crate) error: DriverError,
}

pub(crate) type CompleteTurnResult<A, J, R, T, C, L> = Result<
    CompletedDriverParts<A, J, R, T, C, L>,
    Box<RebindFailure<ProcessAdapterDriver<A, J, R, T, C, L>>>,
>;

pub(crate) type BindTurnResult<A, J, R, T, C, L> = Result<
    ProcessAdapterDriver<A, J, R, T, C, L>,
    Box<RebindFailure<CompletedDriverParts<A, J, R, T, C, L>>>,
>;

pub(crate) struct CompletedDriverParts<A, J, R, T, C, L>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    L: TreeLeaseOwner,
{
    pub(super) completed: CompletedPumpParts<A, J, R>,
    pub(super) transport: T,
    pub(super) control: C,
    pub(super) lease: Option<L>,
    pub(super) owner: RuntimeOwner,
    pub(super) clock: Arc<dyn DriverClock>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PrepareDriverTurnError {
    #[error("the reusable driver no longer owns its live session tree")]
    MissingLease,
    #[error("durable next-Turn preparation failed")]
    Prepare(crate::acp::runtime::CoordinatorError),
    #[error("a duplicate next-Turn intent requires authoritative recovery")]
    Duplicate,
    #[error("the store-minted next-Turn identity is invalid")]
    Identity,
    #[error("the quiescent driver could not bind the prepared Turn")]
    Rebind(DriverError),
}

pub(crate) struct PrepareDriverTurnFailure<P> {
    pub(crate) parts: P,
    pub(crate) recovery: Option<PrepareRecovery>,
    pub(crate) error: PrepareDriverTurnError,
}

pub(crate) struct PreparedDriverTurn<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) driver: ProcessAdapterDriver<A, J, R, T, C, SessionTreeLease>,
    pub(crate) dispatch: ActiveDispatch,
}

impl<A, J, R, T, C, L> CompletedDriverParts<A, J, R, T, C, L>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    L: TreeLeaseOwner,
{
    pub(crate) fn terminal_cursor(&self) -> &crate::acp::store::AdapterTurnCursor {
        self.completed.terminal_cursor()
    }

    pub(crate) fn close_stdin(&mut self) {
        self.transport.close();
    }

    pub(crate) fn write_close_session(&mut self) {
        self.completed.write_close_session(&mut self.transport);
    }

    pub(crate) fn bind_turn(
        self,
        active_turn: ActiveTurnIntent,
    ) -> BindTurnResult<A, J, R, T, C, L> {
        let Self {
            completed,
            transport,
            control,
            lease,
            owner,
            clock,
        } = self;
        match completed.bind_turn(active_turn) {
            Ok(pump) => Ok(ProcessAdapterDriver {
                pump,
                transport,
                control,
                lease,
                owner,
                clock,
                state: DriverState::Running,
                first_delivery_unknown: None,
                durable_mutation_unknown: false,
                teardown_detail: None,
                pending_exit: None,
            }),
            Err(error) => {
                let (completed, _) = *error;
                Err(Box::new(RebindFailure {
                    driver: Self {
                        completed,
                        transport,
                        control,
                        lease,
                        owner,
                        clock,
                    },
                    error: DriverError::NotReusable,
                }))
            }
        }
    }
}

impl<A, J, R, T, C> CompletedDriverParts<A, J, R, T, C, SessionTreeLease>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) async fn park_after_zero(&mut self, token: crate::acp::process::ZeroProcessToken)
    where
        C::ZeroProof: CoordinatorZeroToken,
    {
        let Ok(proof) = token.try_into_coordinator_proof(self.owner.clone()) else {
            return;
        };
        let Some(lease) = self.lease.as_mut() else {
            return;
        };
        let Ok(exit_kind) = crate::acp::contract::RequiredText::new("exitKind", "hostShutdown")
        else {
            return;
        };
        let _ = lease
            .park_after_zero_process(
                crate::acp::store::ParkSessionCommand {
                    project_id: self.owner.project_id.clone(),
                    chat_id: self.owner.chat_id.clone(),
                    session_id: self.owner.session_id.clone(),
                    runtime_epoch: self.owner.runtime_epoch,
                    exit_kind,
                    now_millis: crate::persistence::now_millis(),
                },
                proof,
            )
            .await;
    }

    pub(crate) async fn prepare_and_bind<Auth: ProjectIntentAuthority>(
        self,
        coordinator: &SendCoordinator<Auth>,
        command: PrepareTurnCommand,
    ) -> Result<PreparedDriverTurn<A, J, R, T, C>, Box<PrepareDriverTurnFailure<Self>>> {
        let Some(lease) = self.lease.as_ref() else {
            return Err(Box::new(PrepareDriverTurnFailure {
                parts: self,
                recovery: None,
                error: PrepareDriverTurnError::MissingLease,
            }));
        };
        let prepared = match lease.prepare_turn(coordinator, command).await {
            Ok(prepared) => prepared,
            Err(failure) => {
                let crate::acp::runtime::PrepareFailure { error, recovery } = *failure;
                return Err(Box::new(PrepareDriverTurnFailure {
                    parts: self,
                    recovery,
                    error: PrepareDriverTurnError::Prepare(error),
                }));
            }
        };
        let dispatch = match prepared {
            PrepareDispatch::Applied(dispatch) => dispatch,
            PrepareDispatch::Duplicate(_) => {
                return Err(Box::new(PrepareDriverTurnFailure {
                    parts: self,
                    recovery: None,
                    error: PrepareDriverTurnError::Duplicate,
                }));
            }
        };
        let active_turn = match ActiveTurnIntent::store_minted(dispatch.turn_id().clone()) {
            Ok(active_turn) => active_turn,
            Err(_) => {
                return Err(Box::new(PrepareDriverTurnFailure {
                    parts: self,
                    recovery: Some(PrepareRecovery::Committed(dispatch)),
                    error: PrepareDriverTurnError::Identity,
                }));
            }
        };
        match self.bind_turn(active_turn) {
            Ok(driver) => Ok(PreparedDriverTurn { driver, dispatch }),
            Err(failure) => Err(Box::new(PrepareDriverTurnFailure {
                parts: failure.driver,
                recovery: Some(PrepareRecovery::Committed(dispatch)),
                error: PrepareDriverTurnError::Rebind(failure.error),
            })),
        }
    }
}

pub(crate) struct ZeroProcessHandoff<L, P> {
    pub(crate) lease: L,
    pub(crate) proof: P,
    pub(crate) exit: TreeExit,
    pub(crate) cause: TeardownCause,
    pub(crate) first_delivery_unknown: Option<super::FirstDeliveryUnknown>,
}

impl ZeroProcessHandoff<SessionTreeLease, ZeroProcessToken> {
    pub(crate) fn into_coordinator_proof(
        self,
    ) -> Result<CoordinatorZeroProcessHandoff, DriverError> {
        let owner = self.lease.owner().clone();
        let proof = ZeroProcessProof::from_supervisor(owner, &self.proof)
            .map_err(|_| DriverError::IdentityMismatch)?;
        Ok(CoordinatorZeroProcessHandoff {
            lease: self.lease,
            proof,
            exit: self.exit,
            cause: self.cause,
            first_delivery_unknown: self.first_delivery_unknown,
        })
    }
}

pub(crate) struct CoordinatorZeroProcessHandoff {
    pub(crate) lease: SessionTreeLease,
    pub(crate) proof: ZeroProcessProof,
    pub(crate) exit: TreeExit,
    pub(crate) cause: TeardownCause,
    pub(crate) first_delivery_unknown: Option<super::FirstDeliveryUnknown>,
}
