use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{
        ActiveTurnIntent, AdapterBatchJournal, PrepareRecovery, ProjectIntentAuthority,
        SendCoordinator, SessionTreeLease,
    },
};

use super::{
    CompletedDriverParts, PrepareDriverTurnError, PrepareDriverTurnFailure, PreparedDriverTurn,
};
use crate::acp::runtime::process_driver::{ProcessTreeControl, StdinTransport};

impl<A, J, R, T, C> CompletedDriverParts<A, J, R, T, C, SessionTreeLease>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    /// Opens the Turn the harness started on its own and binds it, so the
    /// stdout it already produced is read as that Turn's.
    pub(crate) async fn open_harness_turn<Auth: ProjectIntentAuthority>(
        self,
        coordinator: &SendCoordinator<Auth>,
        now_millis: i64,
    ) -> Result<PreparedDriverTurn<A, J, R, T, C>, Box<PrepareDriverTurnFailure<Self>>> {
        let Some(lease) = self.lease.as_ref() else {
            return Err(Box::new(PrepareDriverTurnFailure {
                parts: self,
                recovery: None,
                error: PrepareDriverTurnError::MissingLease,
            }));
        };
        let dispatch = match lease.open_harness_turn(coordinator, now_millis).await {
            Ok(dispatch) => dispatch,
            Err(failure) => {
                let crate::acp::runtime::PrepareFailure { error, recovery } = *failure;
                return Err(Box::new(PrepareDriverTurnFailure {
                    parts: self,
                    recovery,
                    error: PrepareDriverTurnError::Prepare(error),
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
        match self.rebind(|completed| completed.bind_harness_turn(active_turn)) {
            Ok(driver) => Ok(PreparedDriverTurn { driver, dispatch }),
            Err(failure) => Err(Box::new(PrepareDriverTurnFailure {
                parts: failure.driver,
                recovery: Some(PrepareRecovery::Committed(dispatch)),
                error: PrepareDriverTurnError::Rebind(failure.error),
            })),
        }
    }
}
