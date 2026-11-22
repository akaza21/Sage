use super::*;

impl<A: HarnessAdapter, R: ResourceMapper, T: StdinTransport, C: ProcessTreeControl>
    CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C>
{
    pub(crate) async fn prepare_first_harness_turn<Auth: ProjectIntentAuthority>(
        self,
        coordinator: &SendCoordinator<Auth>,
        now_millis: i64,
    ) -> Result<PreparedActivationTurn<A, R, T, C>, Box<PrepareActivationTurnFailure<Self>>> {
        let mode = self.parts.harness_turn_authority();
        let dispatch = match self
            .parts
            .journal()
            .lease()
            .open_harness_turn(coordinator, mode, now_millis)
            .await
        {
            Ok(dispatch) => dispatch,
            Err(failure) => {
                return Err(Box::new(PrepareActivationTurnFailure {
                    parts: self,
                    recovery: failure.recovery,
                    error: PrepareActivationTurnError::Prepare(failure.error),
                }));
            }
        };
        let active_turn = match ActiveTurnIntent::store_minted(dispatch.turn_id().clone()) {
            Ok(turn) => turn,
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
