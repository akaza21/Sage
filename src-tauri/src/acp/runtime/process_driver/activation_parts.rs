use std::sync::Arc;

use serde_json::Value;

use crate::acp::{
    bindings::adapter::{AdapterEvent, HarnessAdapter, ResourceMapper},
    runtime::{
        ActiveDispatch, ActiveTurnIntent, AdapterPump, AdapterPumpError,
        CoordinatorActivationHandoff, CoordinatorActivationJournal, IdleActivationParts,
        JournalHandle, PrepareDispatch, PrepareRecovery, ProjectIntentAuthority,
        ProjectionPublisher, RecoveryActivationParts, SendCoordinator, SessionPumpIntent,
        SessionTreeLease, SqliteAdapterBatchJournal, TeardownActivationParts,
    },
    scheduler::RuntimeOwner,
    store::PrepareTurnCommand,
    transport::{IncrementalFrameDecoder, validate_structure},
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
    pub(crate) idle_decoder: IncrementalFrameDecoder,
    pub(crate) transport: T,
    pub(crate) control: C,
    pub(crate) owner: RuntimeOwner,
    pub(crate) clock: Arc<dyn DriverClock>,
}

impl<A, J, R, T, C> CompletedActivationDriverParts<A, J, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
{
    /// Accepts only validated, explicitly ignorable frames that arrive after
    /// activation hands its ready session to the idle live worker.
    pub(crate) fn accept_idle_stdout_chunk(
        &mut self,
        chunk: &[u8],
    ) -> Result<(), AdapterPumpError> {
        let mut frame = self
            .idle_decoder
            .push_chunk(chunk)
            .map_err(|_| AdapterPumpError::Frame)?;
        let mut config_rejected = false;
        loop {
            if let Some(frame) = frame {
                if validate_structure(&frame).is_err() {
                    log_idle_frame_rejection(&frame, "invalid-structure");
                    return Err(AdapterPumpError::Frame);
                }
                match self.parts.ingest_idle_frame(&frame) {
                    Ok(batch) => {
                        let (events, egress) = batch.into_parts();
                        if !egress.is_empty()
                            || !events.iter().all(|event| {
                                matches!(
                                    event,
                                    AdapterEvent::IgnoredLate | AdapterEvent::IgnoredDuplicate
                                )
                            })
                        {
                            log_idle_frame_rejection(&frame, "non-ignorable-event");
                            return Err(AdapterPumpError::Frame);
                        }
                    }
                    Err(
                        crate::acp::bindings::adapter::AdapterRuntimeError::ConfigMutationRejected,
                    ) => {
                        config_rejected = true;
                    }
                    Err(_) => {
                        log_idle_frame_rejection(&frame, "adapter-rejected");
                        return Err(AdapterPumpError::Frame);
                    }
                }
            }
            frame = self
                .idle_decoder
                .drain_pending()
                .map_err(|_| AdapterPumpError::Frame)?;
            if frame.is_none() {
                return if config_rejected {
                    Err(AdapterPumpError::ConfigMutationRejected)
                } else {
                    Ok(())
                };
            }
        }
    }
}

fn log_idle_frame_rejection(frame: &crate::acp::transport::Frame, reason: &'static str) {
    let value = serde_json::from_slice::<Value>(frame.as_bytes()).ok();
    let object = value.as_ref().and_then(Value::as_object);
    let method = object
        .and_then(|object| object.get("method"))
        .and_then(Value::as_str);
    let response_id = object
        .and_then(|object| object.get("id"))
        .and_then(Value::as_u64);
    let error_code = object
        .and_then(|object| object.get("error"))
        .and_then(Value::as_object)
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64);
    tracing::warn!(
        activation_checkpoint = "activation.idle_stdout",
        idle_frame_reason = reason,
        jsonrpc_method = ?method,
        jsonrpc_response_id = ?response_id,
        jsonrpc_error_code = ?error_code,
        "ACP idle handoff rejected inbound frame"
    );
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
        if !self.parts.idle_decoder.is_frame_aligned() || self.parts.parts.config_pending() {
            return Err(Box::new(FirstTurnBindFailure::Retained {
                prepared: self,
                error: FirstTurnBindError::Pump(AdapterPumpError::Intent),
            }));
        }
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
            idle_decoder: _,
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
                        idle_decoder: IncrementalFrameDecoder::new(),
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
