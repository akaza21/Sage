use super::{AdapterError, TerminalOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnPhase {
    Reserved,
    AwaitingAcknowledgment,
    AwaitingModel,
    ModelRunning,
    ModelEnded,
    Settled(TerminalOutcome),
}

/// Prompt delivery, model activity and settlement share one lifecycle. An
/// extension can start model work after delivery but before its RPC returns;
/// an ordinary prompt must first be acknowledged. Neither acknowledgment nor
/// agent_end closes model work: agent_settled owns that boundary.
#[derive(Debug)]
pub(super) struct TurnLifecycle {
    pub(super) owner: u64,
    pub(super) phase: TurnPhase,
    extension: bool,
    prompt_failed: bool,
    pending_aborts: u8,
    abort_accepted: bool,
}

impl TurnLifecycle {
    pub(super) fn new(owner: u64) -> Self {
        Self {
            owner,
            phase: TurnPhase::Reserved,
            extension: false,
            prompt_failed: false,
            pending_aborts: 0,
            abort_accepted: false,
        }
    }

    pub(super) fn mark_extension(&mut self) -> Result<(), AdapterError> {
        if self.phase != TurnPhase::Reserved {
            return Err(AdapterError::EventSequence);
        }
        self.extension = true;
        Ok(())
    }

    pub(super) fn written(&mut self) -> Result<(), AdapterError> {
        if self.phase != TurnPhase::Reserved {
            return Err(AdapterError::Correlation);
        }
        self.phase = if self.extension {
            TurnPhase::AwaitingModel
        } else {
            TurnPhase::AwaitingAcknowledgment
        };
        Ok(())
    }

    /// True means the RPC resolved without starting model work. A failed RPC
    /// still owns any model work it started, until agent_settled arrives.
    pub(super) fn acknowledged(
        &mut self,
        success: bool,
        handled: bool,
    ) -> Result<bool, AdapterError> {
        let resolved_without_model = match self.phase {
            TurnPhase::Reserved | TurnPhase::AwaitingAcknowledgment if !self.extension => {
                self.phase = TurnPhase::AwaitingModel;
                handled || !success
            }
            TurnPhase::AwaitingModel if self.extension => true,
            TurnPhase::ModelRunning | TurnPhase::ModelEnded if self.extension => false,
            _ => return Err(AdapterError::EventSequence),
        };
        self.prompt_failed |= !success;
        Ok(resolved_without_model)
    }

    pub(super) fn start_model(&mut self) -> Result<(), AdapterError> {
        if !matches!(self.phase, TurnPhase::AwaitingModel | TurnPhase::ModelEnded) {
            return Err(AdapterError::EventSequence);
        }
        self.phase = TurnPhase::ModelRunning;
        Ok(())
    }

    /// False denotes a duplicate agent_end.
    pub(super) fn end_model(&mut self) -> Result<bool, AdapterError> {
        match self.phase {
            TurnPhase::ModelEnded => Ok(false),
            TurnPhase::ModelRunning => {
                self.phase = TurnPhase::ModelEnded;
                Ok(true)
            }
            _ => Err(AdapterError::EventSequence),
        }
    }

    pub(super) fn terminal(&self) -> Option<TerminalOutcome> {
        match self.phase {
            TurnPhase::Settled(outcome) => Some(outcome),
            _ => None,
        }
    }

    /// Returns the new outcome, or None for an identical repeated settlement.
    pub(super) fn settle(
        &mut self,
        incoming: TerminalOutcome,
    ) -> Result<Option<TerminalOutcome>, AdapterError> {
        let outcome = self.outcome(incoming);
        match self.terminal() {
            Some(current) if current == outcome => Ok(None),
            Some(_) => Err(AdapterError::TerminalConflict),
            None => {
                self.phase = TurnPhase::Settled(outcome);
                Ok(Some(outcome))
            }
        }
    }

    pub(super) fn request_abort(&mut self) {
        // Every outstanding abort occupies the translator's bounded pending map.
        self.pending_aborts += 1;
    }

    pub(super) fn resolve_abort(&mut self, accepted: bool) {
        self.pending_aborts -= 1;
        self.abort_accepted |= accepted;
    }

    fn outcome(&self, incoming: TerminalOutcome) -> TerminalOutcome {
        if self.abort_accepted || self.pending_aborts > 0 {
            TerminalOutcome::Cancelled
        } else if self.prompt_failed {
            TerminalOutcome::Failed
        } else {
            incoming
        }
    }
}
