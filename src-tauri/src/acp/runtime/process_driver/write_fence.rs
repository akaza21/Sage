use crate::acp::{contract::SageTurnId, store::SendObservation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum WriteFenceError {
    #[error("the write observation belongs to a stale or foreign Turn")]
    Intent,
    #[error("the write fence cannot accept this transition")]
    State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Bound,
    AwaitingObservation,
    StdoutOpen,
    Terminal,
}

/// Orders the native write result before any stdout-derived adapter facts.
/// The worker may still accept reserved control while stdout is fenced.
pub(crate) struct WriteFence {
    turn_id: SageTurnId,
    phase: Phase,
}

impl WriteFence {
    pub(crate) fn bound(turn_id: SageTurnId) -> Self {
        Self {
            turn_id,
            phase: Phase::Bound,
        }
    }

    pub(crate) fn begin_write(&mut self, turn_id: &SageTurnId) -> Result<(), WriteFenceError> {
        self.exact(turn_id)?;
        if self.phase != Phase::Bound {
            return Err(WriteFenceError::State);
        }
        self.phase = Phase::AwaitingObservation;
        Ok(())
    }

    pub(crate) fn accept_observation(
        &mut self,
        turn_id: &SageTurnId,
        observation: SendObservation,
    ) -> Result<(), WriteFenceError> {
        self.exact(turn_id)?;
        if observation == SendObservation::ProcessLost
            && matches!(
                self.phase,
                Phase::Bound | Phase::AwaitingObservation | Phase::StdoutOpen
            )
        {
            self.phase = Phase::Terminal;
            return Ok(());
        }
        if self.phase != Phase::AwaitingObservation {
            return Err(WriteFenceError::State);
        }
        self.phase = match observation {
            SendObservation::ZeroBytes => Phase::Bound,
            SendObservation::CompleteBytes | SendObservation::Correlated => Phase::StdoutOpen,
            SendObservation::PossibleBytes | SendObservation::ProcessLost => Phase::Terminal,
        };
        Ok(())
    }

    pub(crate) fn stdout_allowed(&self) -> bool {
        self.phase == Phase::StdoutOpen
    }

    pub(crate) fn awaiting_observation(&self) -> bool {
        self.phase == Phase::AwaitingObservation
    }

    pub(crate) fn ready_for_write(&self) -> bool {
        self.phase == Phase::Bound
    }

    /// No further write of this Turn's prompt can be admitted.
    pub(crate) fn delivery_settled(&self) -> bool {
        matches!(self.phase, Phase::StdoutOpen | Phase::Terminal)
    }

    fn exact(&self, turn_id: &SageTurnId) -> Result<(), WriteFenceError> {
        if &self.turn_id == turn_id {
            Ok(())
        } else {
            Err(WriteFenceError::Intent)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(value: &str) -> SageTurnId {
        SageTurnId::parse(format!("turn-{value}")).unwrap()
    }

    #[test]
    fn stdout_stays_fenced_until_the_exact_complete_write_is_durable() {
        let active = turn("active");
        let mut fence = WriteFence::bound(active.clone());
        fence.begin_write(&active).unwrap();
        assert!(fence.awaiting_observation());
        assert!(!fence.stdout_allowed());
        assert_eq!(
            fence.accept_observation(&turn("late"), SendObservation::CompleteBytes),
            Err(WriteFenceError::Intent)
        );
        assert!(!fence.stdout_allowed());
        fence
            .accept_observation(&active, SendObservation::CompleteBytes)
            .unwrap();
        assert!(fence.stdout_allowed());
    }

    #[test]
    fn zero_byte_evidence_reopens_only_the_same_turn_for_one_coordinator_retry() {
        let active = turn("active");
        let mut fence = WriteFence::bound(active.clone());
        fence.begin_write(&active).unwrap();
        fence
            .accept_observation(&active, SendObservation::ZeroBytes)
            .unwrap();
        assert!(!fence.stdout_allowed());
        fence.begin_write(&active).unwrap();
        assert!(fence.awaiting_observation());
    }

    #[test]
    fn process_loss_terminalizes_retry_exhaustion_after_the_second_zero_byte() {
        let active = turn("active");
        let mut fence = WriteFence::bound(active.clone());
        for _ in 0..2 {
            fence.begin_write(&active).unwrap();
            fence
                .accept_observation(&active, SendObservation::ZeroBytes)
                .unwrap();
        }
        fence
            .accept_observation(&active, SendObservation::ProcessLost)
            .unwrap();
        assert!(!fence.stdout_allowed());
        assert_eq!(fence.begin_write(&active), Err(WriteFenceError::State));
    }

    #[test]
    fn process_loss_terminalizes_a_prepared_turn_before_its_first_write() {
        let active = turn("active");
        let mut fence = WriteFence::bound(active.clone());
        fence
            .accept_observation(&active, SendObservation::ProcessLost)
            .unwrap();
        assert_eq!(fence.begin_write(&active), Err(WriteFenceError::State));
    }
}
