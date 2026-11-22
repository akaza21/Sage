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
