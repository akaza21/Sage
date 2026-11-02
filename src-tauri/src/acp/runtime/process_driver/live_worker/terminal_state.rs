use super::*;

impl<A, R, T, C> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    pub(crate) fn has_durable_completed_cursor(&self) -> bool {
        self.durable_cursor_is(TurnState::Completed)
    }

    pub(crate) fn has_durable_cancelled_cursor(&self) -> bool {
        matches!(
            self.durable_cursor(),
            Some(TurnState::Cancelled | TurnState::Interrupted)
        )
    }

    fn durable_cursor_is(&self, expected: TurnState) -> bool {
        self.durable_cursor() == Some(expected)
    }

    fn durable_cursor(&self) -> Option<TurnState> {
        match self {
            Self::Reusable(parts) => Some(parts.terminal_cursor().state()),
            Self::ReusableDraining(closing) => Some(closing.terminal_cursor().state()),
            Self::ReusableZero(zero) => Some(zero.terminal_cursor().state()),
            Self::Active(active) => Some(
                active
                    .turn
                    .driver
                    .latest_durable_turn_cursor()
                    .filter(|cursor| {
                        cursor.turn_id() == active.turn.dispatch.turn_id()
                            && matches!(
                                cursor.state(),
                                TurnState::Cancelled | TurnState::Interrupted
                            )
                    })
                    // SQLite can commit cancellation before the host's callback
                    // advances dispatch. Shutdown must honor that durable fact.
                    .map_or_else(|| active.durable_turn_state(), |cursor| cursor.state()),
            ),
            Self::Recovery(recovery) => match recovery.as_ref() {
                FirstTurnWorkerRecovery::Terminal { cursor, .. } => Some(cursor.state()),
                FirstTurnWorkerRecovery::ReusableBegin(failure) => {
                    Some(failure.state.terminal_cursor().state())
                }
                FirstTurnWorkerRecovery::ReusableConverge(failure) => {
                    Some(failure.state.terminal_cursor().state())
                }
                FirstTurnWorkerRecovery::ReusableFinalize(failure) => {
                    Some(failure.state.terminal_cursor().state())
                }
                _ => None,
            },
            _ => None,
        }
    }
}
