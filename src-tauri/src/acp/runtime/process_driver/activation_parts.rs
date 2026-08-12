use std::sync::Arc;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{
        ActiveDispatch, ActiveTurnIntent, AdapterPump, AdapterPumpError,
        CoordinatorActivationHandoff, CoordinatorActivationJournal, IdleActivationParts,
        JournalHandle, PrepareDispatch, PrepareRecovery, ProjectIntentAuthority,
        ProjectionPublisher, RecoveryActivationParts, SendCoordinator, SessionPumpIntent,
        SessionTreeLease, SqliteAdapterBatchJournal, TeardownActivationParts,
    },
    scheduler::RuntimeOwner,
    store::PrepareTurnCommand,
};

use super::{
    ActivationDriverError, DriverClock, DriverError, FirstDeliveryUnknown, PreparedDriverTurn,
    ProcessAdapterDriver, ProcessTreeControl, StdinTransport, TeardownCause,
};

pub(crate) struct ActivationRebindFailure<D> {
    pub(crate) driver: D,
    pub(crate) error: ActivationDriverError,
}

pub(crate) type ActivationDriverHandoff<T, D> = Result<T, Box<ActivationRebindFailure<D>>>;

pub(crate) struct CompletedActivationDriverParts<A, J, R, T, C> {
    pub(crate) parts: IdleActivationParts<A, J, R>,
    pub(crate) transport: T,
    pub(crate) control: C,
    pub(crate) owner: RuntimeOwner,
    pub(crate) clock: Arc<dyn DriverClock>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PrepareActivationTurnError {
    #[error("durable first-Turn preparation failed")]
    Prepare(crate::acp::runtime::CoordinatorError),
    #[error("a duplicate first-Turn intent requires authoritative recovery")]
    Duplicate,
    #[error("the store-minted first-Turn identity is invalid")]
    Identity,
}

pub(crate) struct PrepareActivationTurnFailure<D> {
    pub(crate) parts: D,
    pub(crate) recovery: Option<PrepareRecovery>,
    pub(crate) error: PrepareActivationTurnError,
}

pub(crate) struct PreparedActivationTurn<A, R, T, C> {
    pub(crate) parts: CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C>,
    pub(crate) dispatch: ActiveDispatch,
    pub(crate) active_turn: ActiveTurnIntent,
}

pub(crate) enum FirstTurnBindError {
    Pump(AdapterPumpError),
    ActivationJournal,
    ActivationState(CoordinatorActivationHandoff),
    Driver(DriverError),
}

pub(crate) struct FirstTurnRecoveryParts<A, R, T, C> {
    pub(crate) adapter: A,
    pub(crate) resources: R,
    pub(crate) session: crate::acp::runtime::ActivatedSession,
    pub(crate) lease: SessionTreeLease,
    pub(crate) transport: T,
    pub(crate) control: C,
    pub(crate) owner: RuntimeOwner,
    pub(crate) clock: Arc<dyn DriverClock>,
    pub(crate) dispatch: ActiveDispatch,
    pub(crate) active_turn: ActiveTurnIntent,
    pub(crate) error: FirstTurnBindError,
}

pub(crate) enum FirstTurnBindFailure<A, R, T, C> {
    Retained {
        prepared: PreparedActivationTurn<A, R, T, C>,
        error: FirstTurnBindError,
    },
    Recovery(FirstTurnRecoveryParts<A, R, T, C>),
}

pub(crate) type FirstTurnDriverResult<A, R, T, C> = Result<
    PreparedDriverTurn<A, SqliteAdapterBatchJournal, R, T, C>,
    Box<FirstTurnBindFailure<A, R, T, C>>,
>;

impl<A, R, T, C> CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C> {
    pub(crate) async fn prepare_first_turn<Auth: ProjectIntentAuthority>(
        self,
        coordinator: &SendCoordinator<Auth>,
        command: PrepareTurnCommand,
    ) -> Result<PreparedActivationTurn<A, R, T, C>, Box<PrepareActivationTurnFailure<Self>>> {
        let prepared = match self
            .parts
            .journal()
            .lease()
            .prepare_turn(coordinator, command)
            .await
        {
            Ok(prepared) => prepared,
            Err(failure) => {
                let crate::acp::runtime::PrepareFailure { error, recovery } = *failure;
                return Err(Box::new(PrepareActivationTurnFailure {
                    parts: self,
                    recovery,
                    error: PrepareActivationTurnError::Prepare(error),
                }));
            }
        };
        let dispatch = match prepared {
            PrepareDispatch::Applied(dispatch) => dispatch,
            PrepareDispatch::Duplicate(_) => {
                return Err(Box::new(PrepareActivationTurnFailure {
                    parts: self,
                    recovery: None,
                    error: PrepareActivationTurnError::Duplicate,
                }));
            }
        };
        let active_turn = match ActiveTurnIntent::store_minted(dispatch.turn_id().clone()) {
            Ok(active_turn) => active_turn,
            Err(_) => {
                return Err(Box::new(PrepareActivationTurnFailure {
                    parts: self,
                    recovery: Some(PrepareRecovery::Committed(dispatch)),
                    error: PrepareActivationTurnError::Identity,
                }));
            }
        };
        Ok(PreparedActivationTurn {
            parts: self,
            dispatch,
            active_turn,
        })
    }
}

impl<A, R, T, C> PreparedActivationTurn<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) fn bind_driver(
        self,
        journal: JournalHandle,
        publisher: Arc<dyn ProjectionPublisher>,
    ) -> FirstTurnDriverResult<A, R, T, C> {
        #[cfg(feature = "acp-fixture")]
        let characterization_observation = self
            .parts
            .parts
            .journal()
            .lease()
            .characterization_observation();
        let session = self.parts.parts.session();
        let pump_intent = SessionPumpIntent {
            binding_digest: session.binding_digest,
            project_id: session.project_id.clone(),
            chat_id: session.chat_id.clone(),
            session_id: session.session_id.clone(),
            external_session_id: session.external_session_id.clone(),
            runtime_epoch: session.runtime_epoch,
        }
        .bind_turn(self.active_turn.clone());
        let pump_admission =
            match AdapterPump::<A, SqliteAdapterBatchJournal, R>::admit(&pump_intent) {
                Ok(admission) => admission,
                Err(error) => {
                    return Err(Box::new(FirstTurnBindFailure::Retained {
                        prepared: self,
                        error: FirstTurnBindError::Pump(error),
                    }));
                }
            };

        let PreparedActivationTurn {
            parts,
            dispatch,
            active_turn,
        } = self;
        let CompletedActivationDriverParts {
            parts,
            transport,
            control,
            owner,
            clock,
        } = parts;
        let (adapter, activation_journal, resources, session) = parts.into_parts();
        let (lease, handoff) = match activation_journal.into_handoff() {
            Ok(value) => value,
            Err(activation_journal) => {
                let prepared = PreparedActivationTurn {
                    parts: CompletedActivationDriverParts {
                        parts: IdleActivationParts::new(
                            adapter,
                            *activation_journal,
                            resources,
                            session,
                        ),
                        transport,
                        control,
                        owner,
                        clock,
                    },
                    dispatch,
                    active_turn,
                };
                return Err(Box::new(FirstTurnBindFailure::Retained {
                    prepared,
                    error: FirstTurnBindError::ActivationJournal,
                }));
            }
        };
        if handoff != CoordinatorActivationHandoff::Ready {
            return Err(Box::new(FirstTurnBindFailure::Recovery(
                FirstTurnRecoveryParts {
                    adapter,
                    resources,
                    session,
                    lease,
                    transport,
                    control,
                    owner,
                    clock,
                    dispatch,
                    active_turn,
                    error: FirstTurnBindError::ActivationState(handoff),
                },
            )));
        }
        let driver_admission = match ProcessAdapterDriver::<
            A,
            SqliteAdapterBatchJournal,
            R,
            T,
            C,
            SessionTreeLease,
        >::admit(&lease, &owner)
        {
            Ok(admission) => admission,
            Err(error) => {
                return Err(Box::new(FirstTurnBindFailure::Recovery(
                    FirstTurnRecoveryParts {
                        adapter,
                        resources,
                        session,
                        lease,
                        transport,
                        control,
                        owner,
                        clock,
                        dispatch,
                        active_turn,
                        error: FirstTurnBindError::Driver(error),
                    },
                )));
            }
        };
        let adapter_journal = SqliteAdapterBatchJournal::new(journal, pump_intent.clone());
        #[cfg(feature = "acp-fixture")]
        let adapter_journal = match characterization_observation {
            Some(observation) => adapter_journal.with_characterization_observation(observation),
            None => adapter_journal,
        };
        let pump = AdapterPump::from_admission(
            adapter,
            adapter_journal,
            resources,
            publisher,
            pump_intent,
            pump_admission,
        );
        Ok(PreparedDriverTurn {
            driver: ProcessAdapterDriver::from_admission(
                pump,
                transport,
                control,
                lease,
                owner,
                clock,
                driver_admission,
            ),
            dispatch,
        })
    }
}

pub(crate) struct RecoveryActivationDriverParts<A, J, R, T, C> {
    pub(crate) parts: RecoveryActivationParts<A, J, R>,
    pub(crate) transport: T,
    pub(crate) control: C,
    pub(crate) owner: RuntimeOwner,
    pub(crate) clock: Arc<dyn DriverClock>,
    pub(crate) first_delivery_unknown: Option<FirstDeliveryUnknown>,
}

pub(crate) struct ActivationZeroProcessHandoff<A, J, R, P> {
    pub(crate) parts: TeardownActivationParts<A, J, R>,
    pub(crate) proof: P,
    pub(crate) exit: crate::acp::process::TreeExit,
    pub(crate) cause: TeardownCause,
    pub(crate) owner: RuntimeOwner,
    pub(crate) first_delivery_unknown: Option<FirstDeliveryUnknown>,
}

pub(crate) struct ActivationProjectionRecoveryHandoff<A, J, R, P> {
    pub(crate) parts: RecoveryActivationParts<A, J, R>,
    pub(crate) proof: P,
    pub(crate) owner: RuntimeOwner,
    pub(crate) first_delivery_unknown: Option<FirstDeliveryUnknown>,
}
