use super::*;

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

impl<A: HarnessAdapter, R: ResourceMapper, T: StdinTransport, C: ProcessTreeControl>
    PreparedActivationTurn<A, R, T, C>
{
    pub(crate) fn bind_driver(
        mut self,
        journal: JournalHandle,
        publisher: Arc<dyn ProjectionPublisher>,
    ) -> FirstTurnDriverResult<A, R, T, C> {
        // Preparation reserved the prompt's Turn. Idle work arriving before
        // dispatch must be declined, including starts still in the decoder.
        while self.parts.parts.turn_started() {
            let result = self
                .parts
                .parts
                .decline_started_turn(&mut self.parts.transport)
                .and_then(|()| self.parts.accept_idle_stdout_chunk(&[]));
            if let Err(error) = result {
                return Err(Box::new(FirstTurnBindFailure::Retained {
                    prepared: self,
                    error: FirstTurnBindError::Pump(error),
                }));
            }
        }
        self.bind_driver_for(journal, publisher, false)
    }

    pub(crate) fn bind_harness_driver(
        self,
        journal: JournalHandle,
        publisher: Arc<dyn ProjectionPublisher>,
    ) -> FirstTurnDriverResult<A, R, T, C> {
        self.bind_driver_for(journal, publisher, true)
    }
}
